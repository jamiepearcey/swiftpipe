//! In-memory job status store backed by `Arc<RwLock<HashMap>>`.
//!
//! `JobStore` is `Send + Sync` and can be shared freely across tokio tasks and
//! threads. All mutations go through `RwLock` so callers never need to hold a
//! lock across an `await` point.

use std::collections::HashMap;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::{Duration, Instant};

use serde::Serialize;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// The lifecycle state of a single async job.
pub(crate) enum JobStatus {
    Queued,
    Running { started_at: Instant },
    Completed { manifest: serde_json::Value },
    Failed { error: String },
}

/// A serialisable snapshot of a job that callers can hand back as JSON without
/// holding the store lock.
#[derive(Debug, Serialize)]
pub(crate) struct JobStatusView {
    pub(crate) job_id: String,
    /// One of `"queued"`, `"running"`, `"completed"`, `"failed"`.
    pub(crate) status: &'static str,
    /// Milliseconds since the Unix epoch at which the job was first inserted.
    pub(crate) queued_at_ms: Option<u64>,
    /// Milliseconds since the Unix epoch at which the job moved to `Running`.
    pub(crate) started_at_ms: Option<u64>,
    /// For running jobs: how many ms the job has been running so far.
    /// For completed/failed jobs: total elapsed ms from `Running` to terminal.
    pub(crate) elapsed_ms: Option<u64>,
    pub(crate) error: Option<String>,
    pub(crate) manifest: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Internal entry stored inside the map
// ---------------------------------------------------------------------------

struct Entry {
    job_id: String,
    queued_at: Instant,
    status: JobStatus,
    /// Set the first time the entry transitions to `Running`.
    started_at: Option<Instant>,
    /// Set once the job reaches a terminal state (`Completed` or `Failed`).
    finished_at: Option<Instant>,
}

impl Entry {
    fn new(job_id: String, status: JobStatus) -> Self {
        let now = Instant::now();
        let started_at = if let JobStatus::Running { started_at } = &status {
            Some(*started_at)
        } else {
            None
        };
        Self {
            job_id,
            queued_at: now,
            status,
            started_at,
            finished_at: None,
        }
    }

    fn to_view(&self) -> JobStatusView {
        let now = Instant::now();
        let status_str: &'static str = match &self.status {
            JobStatus::Queued => "queued",
            JobStatus::Running { .. } => "running",
            JobStatus::Completed { .. } => "completed",
            JobStatus::Failed { .. } => "failed",
        };

        let queued_at_ms = Some(0u64); // always the reference point

        let started_at_ms = self.started_at.map(|t| duration_ms(self.queued_at, t));

        let elapsed_ms = match &self.status {
            JobStatus::Running { started_at } => Some(duration_ms(*started_at, now)),
            JobStatus::Completed { .. } | JobStatus::Failed { .. } => self.started_at.map(|s| {
                self.finished_at
                    .map_or_else(|| duration_ms(s, now), |f| duration_ms(s, f))
            }),
            JobStatus::Queued => None,
        };

        let error = if let JobStatus::Failed { error } = &self.status {
            Some(error.clone())
        } else {
            None
        };

        let manifest = if let JobStatus::Completed { manifest } = &self.status {
            Some(manifest.clone())
        } else {
            None
        };

        JobStatusView {
            job_id: self.job_id.clone(),
            status: status_str,
            queued_at_ms,
            started_at_ms,
            elapsed_ms,
            error,
            manifest,
        }
    }
}

fn duration_ms(from: Instant, to: Instant) -> u64 {
    to.saturating_duration_since(from)
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

// ---------------------------------------------------------------------------
// JobStore
// ---------------------------------------------------------------------------

pub(crate) struct JobStore {
    // Vec preserves insertion order so `list_recent` can cheaply return the
    // newest entries. The HashMap provides O(1) lookup by job_id.
    inner: RwLock<Inner>,
}

struct Inner {
    map: HashMap<String, usize>, // job_id → index into `entries`
    entries: Vec<Entry>,
}

impl JobStore {
    /// Create a new, empty store wrapped in an `Arc`.
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: RwLock::new(Inner {
                map: HashMap::new(),
                entries: Vec::new(),
            }),
        })
    }

    /// Insert a new job. If a job with the same `job_id` already exists it is
    /// silently overwritten.
    pub(crate) fn insert(&self, job_id: String, status: JobStatus) {
        let mut guard = self.write_inner();
        let entry = Entry::new(job_id.clone(), status);
        if let Some(&idx) = guard.map.get(&job_id) {
            guard.entries[idx] = entry;
        } else {
            let idx = guard.entries.len();
            guard.entries.push(entry);
            guard.map.insert(job_id, idx);
        }
    }

    /// Update the status of an existing job. Returns `false` if no job with
    /// that `job_id` exists (caller's responsibility to `insert` first).
    pub(crate) fn update(&self, job_id: &str, status: JobStatus) -> bool {
        let mut guard = self.write_inner();
        let Some(&idx) = guard.map.get(job_id) else {
            return false;
        };
        let entry = &mut guard.entries[idx];

        // Capture the start timestamp when we first see a Running transition.
        if let JobStatus::Running { started_at } = &status {
            if entry.started_at.is_none() {
                entry.started_at = Some(*started_at);
            }
        }

        // Record when we reach a terminal state.
        if matches!(
            status,
            JobStatus::Completed { .. } | JobStatus::Failed { .. }
        ) {
            entry.finished_at = Some(Instant::now());
        }

        entry.status = status;
        true
    }

    /// Return a serialisable snapshot of a single job, or `None` if unknown.
    pub(crate) fn get(&self, job_id: &str) -> Option<JobStatusView> {
        let guard = self.read_inner();
        let &idx = guard.map.get(job_id)?;
        Some(guard.entries[idx].to_view())
    }

    /// Return up to `limit` jobs in newest-first order.
    pub(crate) fn list_recent(&self, limit: usize) -> Vec<JobStatusView> {
        let guard = self.read_inner();
        guard
            .entries
            .iter()
            .rev()
            .take(limit)
            .map(Entry::to_view)
            .collect()
    }

    /// Return job IDs of jobs that have been in `Running` state longer than
    /// `timeout`. Used by the reaper to mark abandoned jobs as failed.
    pub(crate) fn list_stuck(&self, timeout: Duration) -> Vec<String> {
        let guard = self.read_inner();
        let now = Instant::now();
        guard
            .entries
            .iter()
            .filter(|e| {
                matches!(e.status, JobStatus::Running { .. })
                    && e.started_at
                        .is_some_and(|s| now.saturating_duration_since(s) > timeout)
            })
            .map(|e| e.job_id.clone())
            .collect()
    }

    fn read_inner(&self) -> RwLockReadGuard<'_, Inner> {
        self.inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn write_inner(&self) -> RwLockWriteGuard<'_, Inner> {
        self.inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_store() -> Arc<JobStore> {
        JobStore::new()
    }

    #[test]
    fn insert_and_get_queued_job() {
        let store = make_store();
        store.insert("job-1".to_string(), JobStatus::Queued);

        let view = store.get("job-1").expect("job should exist");
        assert_eq!(view.job_id, "job-1");
        assert_eq!(view.status, "queued");
        assert!(view.error.is_none());
        assert!(view.manifest.is_none());
        assert!(view.elapsed_ms.is_none());
    }

    #[test]
    fn update_to_running_then_completed() {
        let store = make_store();
        store.insert("job-2".to_string(), JobStatus::Queued);

        let found = store.update(
            "job-2",
            JobStatus::Running {
                started_at: Instant::now(),
            },
        );
        assert!(found, "update should return true for a known job");

        let view = store.get("job-2").unwrap();
        assert_eq!(view.status, "running");
        assert!(
            view.elapsed_ms.is_some(),
            "running job should have elapsed_ms"
        );

        let manifest_val = serde_json::json!({"ok": true});
        store.update(
            "job-2",
            JobStatus::Completed {
                manifest: manifest_val.clone(),
            },
        );

        let view = store.get("job-2").unwrap();
        assert_eq!(view.status, "completed");
        assert_eq!(view.manifest.as_ref().unwrap(), &manifest_val);
        assert!(view.error.is_none());
    }

    #[test]
    fn update_returns_false_for_unknown_job() {
        let store = make_store();
        let found = store.update("ghost", JobStatus::Queued);
        assert!(!found, "update should return false for unknown job");
    }

    #[test]
    fn failed_job_carries_error_message() {
        let store = make_store();
        store.insert("job-3".to_string(), JobStatus::Queued);
        store.update(
            "job-3",
            JobStatus::Failed {
                error: "something went wrong".to_string(),
            },
        );

        let view = store.get("job-3").unwrap();
        assert_eq!(view.status, "failed");
        assert_eq!(view.error.as_deref(), Some("something went wrong"));
        assert!(view.manifest.is_none());
    }

    #[test]
    fn list_recent_returns_newest_first_and_respects_limit() {
        let store = make_store();
        for i in 1..=5u32 {
            store.insert(format!("job-{i}"), JobStatus::Queued);
        }

        let recent = store.list_recent(3);
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].job_id, "job-5");
        assert_eq!(recent[1].job_id, "job-4");
        assert_eq!(recent[2].job_id, "job-3");
    }

    #[test]
    fn list_recent_with_limit_larger_than_store_returns_all() {
        let store = make_store();
        store.insert("only-job".to_string(), JobStatus::Queued);

        let all = store.list_recent(100);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].job_id, "only-job");
    }

    #[test]
    fn get_unknown_job_returns_none() {
        let store = make_store();
        assert!(store.get("no-such-job").is_none());
    }

    #[test]
    fn list_stuck_returns_jobs_past_timeout() {
        let store = make_store();
        // Insert a job and immediately mark it running with a very old started_at.
        store.insert("old-job".to_string(), JobStatus::Queued);
        store.update(
            "old-job",
            JobStatus::Running {
                // Simulate started 10 minutes ago.
                started_at: Instant::now()
                    .checked_sub(Duration::from_secs(600))
                    .expect("test duration is valid"),
            },
        );
        store.insert("fresh-job".to_string(), JobStatus::Queued);
        store.update(
            "fresh-job",
            JobStatus::Running {
                started_at: Instant::now(),
            },
        );

        let stuck = store.list_stuck(Duration::from_secs(300));
        assert_eq!(stuck, vec!["old-job".to_string()]);
    }
}
