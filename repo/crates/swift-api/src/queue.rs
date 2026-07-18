//! Bounded async job queue with a tokio worker pool.
//!
//! `JobQueue::spawn` starts `num_workers` long-running tokio tasks that each
//! pull from a shared bounded mpsc channel. CPU/IO-bound processing happens
//! inside `tokio::task::spawn_blocking` so the async runtime stays free.
//!
//! # Shutdown
//! Drop all `JobQueue` handles (or just the one returned by `spawn`). The
//! workers will drain any remaining messages and then exit, at which point the
//! `JoinSet` returned by `spawn` will become empty.

use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use tokio::sync::mpsc;
use tokio::task::JoinSet;

use crate::job_store::{JobStatus, JobStore};
use crate::manifest::JobRequest;
use crate::state::{AppState, Metrics};

// ---------------------------------------------------------------------------
// Public task enum
// ---------------------------------------------------------------------------

/// A unit of work submitted to the queue.
pub(crate) enum JobTask {
    /// A raw FIN upload to be processed via `crate::job::process_upload`.
    Upload {
        job_id: String,
        body: Vec<u8>,
        message_type: Option<String>,
        outputs: Option<Vec<String>>,
    },
    /// A structured batch request to be processed via
    /// `crate::job::process_job_request`.
    Batch { job_id: String, request: JobRequest },
}

impl JobTask {
    fn job_id(&self) -> &str {
        match self {
            JobTask::Upload { job_id, .. } | JobTask::Batch { job_id, .. } => job_id,
        }
    }
}

// ---------------------------------------------------------------------------
// JobQueue
// ---------------------------------------------------------------------------

/// The sender side of the bounded job queue.
///
/// Clone cheaply; all clones share the same underlying channel.
#[derive(Clone)]
pub(crate) struct JobQueue {
    sender: mpsc::Sender<JobTask>,
    metrics: Arc<Metrics>,
}

impl JobQueue {
    #[allow(dead_code)]
    pub(crate) fn from_sender_for_test(
        sender: mpsc::Sender<JobTask>,
        metrics: Arc<Metrics>,
    ) -> Self {
        Self { sender, metrics }
    }

    /// Start `num_workers` tokio tasks consuming from a channel with capacity
    /// `capacity`.
    ///
    /// Returns:
    /// * The `JobQueue` (sender side) for submitting tasks.
    /// * A `JoinSet` you can `await` on shutdown to wait for all workers to
    ///   drain and exit.
    pub(crate) fn spawn(
        num_workers: usize,
        capacity: usize,
        state: Arc<AppState>,
        job_store: Arc<JobStore>,
    ) -> (Self, JoinSet<()>) {
        assert!(num_workers > 0, "num_workers must be at least 1");
        assert!(capacity > 0, "capacity must be at least 1");

        let (sender, receiver) = mpsc::channel::<JobTask>(capacity);
        // Wrap the receiver in an Arc<Mutex> so each worker can hold a shared
        // reference without requiring a separate clone of the receiver (which
        // mpsc does not support).
        let receiver = Arc::new(tokio::sync::Mutex::new(receiver));

        let mut join_set = JoinSet::new();
        for _ in 0..num_workers {
            let rx = Arc::clone(&receiver);
            let state = Arc::clone(&state);
            let store = Arc::clone(&job_store);
            join_set.spawn(run_worker(rx, state, store));
        }

        (
            Self {
                sender,
                metrics: Arc::clone(&state.metrics),
            },
            join_set,
        )
    }

    pub(crate) fn len(&self) -> usize {
        self.sender.max_capacity() - self.sender.capacity()
    }

    /// Submit a task to the queue.
    ///
    /// Returns `Err` if the queue is full (caller should respond with HTTP 503)
    /// or if all receivers have been dropped (workers have shut down).
    pub(crate) fn submit(&self, task: JobTask) -> Result<(), anyhow::Error> {
        self.sender.try_send(task).map_err(|err| match err {
            mpsc::error::TrySendError::Full(_) => {
                anyhow!("job queue is full; try again later")
            }
            mpsc::error::TrySendError::Closed(_) => {
                anyhow!("job queue has shut down")
            }
        })?;
        self.metrics.set_queue_depth(self.len());
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Worker loop
// ---------------------------------------------------------------------------

async fn run_worker(
    receiver: Arc<tokio::sync::Mutex<mpsc::Receiver<JobTask>>>,
    state: Arc<AppState>,
    job_store: Arc<JobStore>,
) {
    loop {
        // Acquire the lock only long enough to pull one task, then release it
        // so other workers can compete.
        let task = {
            let mut guard = receiver.lock().await;
            guard.recv().await
        };

        let Some(task) = task else {
            // Channel closed and drained — worker should exit.
            break;
        };
        state.metrics.dispatched();

        process_task(task, Arc::clone(&state), Arc::clone(&job_store)).await;
    }
}

async fn process_task(task: JobTask, state: Arc<AppState>, job_store: Arc<JobStore>) {
    let job_id = task.job_id().to_string();
    let started = std::time::Instant::now();

    // Transition to Running before handing off to the blocking thread.
    job_store.update(
        &job_id,
        JobStatus::Running {
            started_at: std::time::Instant::now(),
        },
    );

    let result = match task {
        JobTask::Upload {
            job_id: jid,
            body,
            message_type,
            outputs,
            ..
        } => {
            let state = Arc::clone(&state);
            tokio::task::spawn_blocking(move || {
                crate::job::process_upload(&state, jid, body, message_type, outputs)
            })
            .await
        }

        JobTask::Batch {
            job_id: jid,
            request,
        } => {
            let state = Arc::clone(&state);
            tokio::task::spawn_blocking(move || {
                crate::job::process_job_request(&state, jid, request)
            })
            .await
        }
    };

    // Flatten the two error layers: JoinError (panic) and anyhow::Error.
    let job_result = match result {
        Ok(Ok(manifest)) => Ok(manifest),
        Ok(Err(err)) => Err(err.to_string()),
        Err(join_err) => Err(format!("worker task panicked: {join_err}")),
    };

    let new_status = match job_result {
        Ok(manifest) => match serde_json::to_value(&manifest) {
            Ok(value) => JobStatus::Completed { manifest: value },
            Err(err) => JobStatus::Failed {
                error: format!("failed to serialise manifest: {err}"),
            },
        },
        Err(error) => JobStatus::Failed { error },
    };

    let terminal_status = matches!(new_status, JobStatus::Completed { .. });
    job_store.update(&job_id, new_status);

    // Update metrics so the in-flight gauge stays accurate.
    if terminal_status {
        state.metrics.completed(started.elapsed());
    } else {
        state.metrics.failed(Some(started.elapsed()));
    }

    // Clean up the per-job work directory on completion or failure.
    let work_dir = state.work_root.join(&job_id);
    if work_dir.exists() {
        if let Err(err) = std::fs::remove_dir_all(&work_dir) {
            tracing::warn!(job_id = %job_id, error = %err, "failed to remove work dir");
        }
    }
}

// ---------------------------------------------------------------------------
// Reaper
// ---------------------------------------------------------------------------

/// Spawn a background task that periodically marks abandoned `Running` jobs
/// (those whose worker thread was killed without updating the store) as
/// `Failed`.  The timeout should be generous — longer than the maximum
/// expected job duration.
pub(crate) fn spawn_reaper(
    job_store: Arc<JobStore>,
    metrics: Arc<Metrics>,
    check_interval: Duration,
    stuck_timeout: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(check_interval);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            let stuck = job_store.list_stuck(stuck_timeout);
            for job_id in stuck {
                tracing::warn!(job_id = %job_id, "reaper: marking stuck job as failed");
                job_store.update(
                    &job_id,
                    JobStatus::Failed {
                        error: format!(
                            "job exceeded maximum runtime of {}s and was marked failed by the reaper",
                            stuck_timeout.as_secs()
                        ),
                    },
                );
                metrics.stuck();
            }
        }
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job_store::JobStore;
    use crate::state::{default_idempotency_store, make_rate_limiter};
    use std::num::NonZeroU32;
    use std::time::Duration;

    fn make_state() -> Arc<AppState> {
        use crate::system_record::SystemOfRecordConfig;
        Arc::new(AppState {
            schema_path: std::path::PathBuf::from("/nonexistent/schemas"),
            schema_catalog: crate::state::empty_schema_catalog(),
            object_root: std::path::PathBuf::from("/nonexistent/objects"),
            work_root: std::path::PathBuf::from("/nonexistent/work"),
            max_upload_bytes: 1024,
            max_prefix_fanout: 10_000,
            max_prefix_parallelism: crate::state::DEFAULT_MAX_PREFIX_PARALLELISM,
            zip_max_total_bytes: crate::state::DEFAULT_ZIP_MAX_TOTAL_BYTES,
            zip_max_entries: crate::state::DEFAULT_ZIP_MAX_ENTRIES,
            persist_raw_text: false,
            persist_raw_fields: false,
            request_timeout: Duration::from_secs(120),
            rate_limiter: Arc::new(make_rate_limiter(
                NonZeroU32::new(5).expect("non-zero rps"),
                NonZeroU32::new(20).expect("non-zero burst"),
            )),
            idempotency: Arc::new(default_idempotency_store()),
            system_of_record: SystemOfRecordConfig::None,
            ready_check: None,
            job_store: JobStore::new(),
            job_queue: None,
            metrics: Arc::default(),
        })
    }

    /// Submitting a task when the queue is at capacity should return an error
    /// without blocking.
    #[tokio::test]
    async fn submit_to_full_queue_returns_error() {
        let store = JobStore::new();
        let _state = make_state();
        // Capacity of 1 with 0 workers so nothing is ever consumed.
        let (tx, _rx) = mpsc::channel::<JobTask>(1);
        let queue = JobQueue::from_sender_for_test(tx, Arc::new(Metrics::default()));

        // Fill the single slot.
        store.insert("job-fill".to_string(), JobStatus::Queued);
        let first = queue.submit(JobTask::Upload {
            job_id: "job-fill".to_string(),
            body: vec![],
            message_type: None,
            outputs: None,
        });
        assert!(
            first.is_ok(),
            "first submit into empty queue should succeed"
        );

        // Queue is now full; second submit must fail immediately.
        store.insert("job-overflow".to_string(), JobStatus::Queued);
        let second = queue.submit(JobTask::Upload {
            job_id: "job-overflow".to_string(),
            body: vec![],
            message_type: None,
            outputs: None,
        });
        assert!(second.is_err(), "submit to full queue should return Err");
        let msg = second.unwrap_err().to_string();
        assert!(
            msg.contains("full") || msg.contains("queue"),
            "error message should mention full/queue: {msg}"
        );
    }

    /// Submitting after all receivers are gone should return an error.
    #[tokio::test]
    async fn submit_to_closed_queue_returns_error() {
        let (tx, rx) = mpsc::channel::<JobTask>(4);
        drop(rx); // close the receiver side
        let queue = JobQueue::from_sender_for_test(tx, Arc::new(Metrics::default()));

        let result = queue.submit(JobTask::Upload {
            job_id: "job-closed".to_string(),
            body: vec![],
            message_type: None,
            outputs: None,
        });

        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("shut down") || msg.contains("closed"),
            "error message should mention shutdown/closed: {msg}"
        );
    }

    /// Workers should transition jobs to `Failed` when processing fails
    /// (invalid state path causes an error in `process_upload`).
    #[tokio::test]
    async fn worker_marks_job_failed_on_processing_error() {
        let store = JobStore::new();
        let state = make_state(); // schema_path doesn't exist → process_upload will fail
        let (queue, mut join_set) = JobQueue::spawn(1, 8, Arc::clone(&state), Arc::clone(&store));

        store.insert("job-fail".to_string(), JobStatus::Queued);
        queue
            .submit(JobTask::Upload {
                job_id: "job-fail".to_string(),
                body: b"not a real FIN".to_vec(),
                message_type: Some("MT540".to_string()),
                outputs: None,
            })
            .expect("submit should succeed");

        // Drop the queue to close the channel so the worker exits after
        // processing the one pending task.
        drop(queue);

        // Wait for all workers to finish (with a generous timeout).
        let timeout = tokio::time::timeout(Duration::from_secs(10), async {
            while join_set.join_next().await.is_some() {}
        });
        timeout.await.expect("workers should finish within timeout");

        let view = store.get("job-fail").expect("job should be in store");
        assert_eq!(
            view.status, "failed",
            "job should be marked failed when processing errors"
        );
        assert!(
            view.error.is_some(),
            "failed job should carry an error message"
        );
    }

    /// `list_recent` on the store reflects the order in which jobs were
    /// inserted, newest first.
    #[tokio::test]
    async fn job_store_tracks_multiple_jobs_in_insertion_order() {
        let store = JobStore::new();
        for i in 1..=4u32 {
            store.insert(format!("j{i}"), JobStatus::Queued);
        }
        let recent = store.list_recent(2);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].job_id, "j4");
        assert_eq!(recent[1].job_id, "j3");
    }
}
