#![forbid(unsafe_code)]
#![deny(warnings, rust_2018_idioms, missing_debug_implementations)]
#![warn(clippy::pedantic)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    dead_code,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::needless_pass_by_value,
    clippy::similar_names,
    clippy::struct_excessive_bools,
    clippy::struct_field_names,
    clippy::too_many_lines
)]

use axum::Router;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinSet;

mod auth;
mod error;
mod job;
mod job_store;
mod manifest;
mod object_store;
mod queue;
mod routes;
mod state;
mod system_record;
mod ui;

use job_store::JobStore;
use queue::{JobQueue, JobTask};
use routes::make_router;
use state::{
    default_idempotency_store, make_rate_limiter, AppState, Metrics,
    DEFAULT_MAX_PREFIX_PARALLELISM, DEFAULT_ZIP_MAX_ENTRIES, DEFAULT_ZIP_MAX_TOTAL_BYTES,
};
use system_record::{ReadyCheck, SystemOfRecordConfig};

#[derive(Debug, Clone)]
pub enum TestJobStatus {
    Queued,
    Failed { error: String },
    Completed { manifest: serde_json::Value },
}

#[derive(Debug, Clone)]
pub struct TestJobStatusSnapshot {
    pub job_id: String,
    pub status: String,
    pub error: Option<String>,
}

pub struct QueuedTestWorkers {
    job_store: Arc<JobStore>,
    worker_join_set: JoinSet<()>,
}

impl std::fmt::Debug for QueuedTestWorkers {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("QueuedTestWorkers")
            .finish_non_exhaustive()
    }
}

impl QueuedTestWorkers {
    pub fn list_jobs(&self, limit: usize) -> Vec<TestJobStatusSnapshot> {
        self.job_store
            .list_recent(limit)
            .into_iter()
            .map(|job| TestJobStatusSnapshot {
                job_id: job.job_id,
                status: job.status.to_string(),
                error: job.error,
            })
            .collect()
    }

    pub async fn drain(&mut self, timeout: Duration) -> Result<(), String> {
        while !self.worker_join_set.is_empty() {
            let joined = tokio::time::timeout(timeout, self.worker_join_set.join_next())
                .await
                .map_err(|_| format!("worker JoinSet did not drain within {timeout:?}"))?;
            if joined.is_none() {
                break;
            }
        }
        Ok(())
    }
}

pub fn make_sync_router_for_test(
    schema_path: PathBuf,
    object_root: PathBuf,
    work_root: PathBuf,
    max_upload_bytes: usize,
) -> Router {
    make_sync_router_with_jobs_for_test(
        schema_path,
        object_root,
        work_root,
        max_upload_bytes,
        Vec::new(),
    )
}

pub fn job_request_field_names_for_test() -> &'static [&'static str] {
    manifest::JOB_REQUEST_FIELDS
}

#[doc(hidden)]
pub fn write_zip_for_bench(
    object_root: &Path,
    prefix_uri: &str,
    zip_uri: &str,
    max_total_bytes: u64,
    max_entries: usize,
) -> Result<(), String> {
    job::write_zip_for_bench(
        object_root,
        prefix_uri,
        zip_uri,
        max_total_bytes,
        max_entries,
    )
    .map_err(|error| error.to_string())
}

fn load_schema_catalog(schema_path: &Path) -> Arc<swift_schema::SchemaCatalog> {
    if !schema_path.exists() {
        return Arc::new(swift_schema::SchemaCatalog {
            field_types: Vec::new(),
            messages: Vec::new(),
        });
    }
    let catalog = job::load_catalog(std::slice::from_ref(&schema_path.to_path_buf()))
        .expect("test schema catalog loads");
    catalog
        .validate_rendering()
        .expect("test schema catalog validates rendering");
    Arc::new(catalog)
}

pub fn make_sync_router_with_jobs_for_test(
    schema_path: PathBuf,
    object_root: PathBuf,
    work_root: PathBuf,
    max_upload_bytes: usize,
    jobs: Vec<(String, TestJobStatus)>,
) -> Router {
    let job_store = JobStore::new();
    for (job_id, status) in jobs {
        job_store.insert(job_id, test_status(status));
    }
    let schema_catalog = load_schema_catalog(&schema_path);
    let state = Arc::new(AppState {
        schema_path,
        schema_catalog,
        object_root,
        work_root,
        max_upload_bytes,
        max_prefix_fanout: 10_000,
        max_prefix_parallelism: DEFAULT_MAX_PREFIX_PARALLELISM,
        zip_max_total_bytes: DEFAULT_ZIP_MAX_TOTAL_BYTES,
        zip_max_entries: DEFAULT_ZIP_MAX_ENTRIES,
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
        job_store,
        job_queue: None,
        metrics: Arc::new(Metrics::default()),
    });
    make_router(state, None)
}

pub fn make_queued_router_for_test(
    schema_path: PathBuf,
    object_root: PathBuf,
    work_root: PathBuf,
    max_upload_bytes: usize,
    job_workers: usize,
    job_queue_capacity: usize,
) -> (Router, QueuedTestWorkers) {
    let job_store = JobStore::new();
    let metrics = Arc::new(Metrics::default());
    let schema_catalog = load_schema_catalog(&schema_path);
    let worker_state = Arc::new(AppState {
        schema_path: schema_path.clone(),
        schema_catalog: Arc::clone(&schema_catalog),
        object_root: object_root.clone(),
        work_root: work_root.clone(),
        max_upload_bytes,
        max_prefix_fanout: 10_000,
        max_prefix_parallelism: DEFAULT_MAX_PREFIX_PARALLELISM,
        zip_max_total_bytes: DEFAULT_ZIP_MAX_TOTAL_BYTES,
        zip_max_entries: DEFAULT_ZIP_MAX_ENTRIES,
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
        job_store: Arc::clone(&job_store),
        job_queue: None,
        metrics: Arc::clone(&metrics),
    });
    let (queue, worker_join_set) = JobQueue::spawn(
        job_workers,
        job_queue_capacity,
        Arc::clone(&worker_state),
        Arc::clone(&job_store),
    );
    let state = Arc::new(AppState {
        schema_path,
        schema_catalog,
        object_root,
        work_root,
        max_upload_bytes,
        max_prefix_fanout: 10_000,
        max_prefix_parallelism: DEFAULT_MAX_PREFIX_PARALLELISM,
        zip_max_total_bytes: DEFAULT_ZIP_MAX_TOTAL_BYTES,
        zip_max_entries: DEFAULT_ZIP_MAX_ENTRIES,
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
        job_store: Arc::clone(&job_store),
        job_queue: Some(queue),
        metrics,
    });
    (
        make_router(state, None),
        QueuedTestWorkers {
            job_store,
            worker_join_set,
        },
    )
}

pub fn make_sync_router_with_postgres_ready_check_for_test(
    schema_path: PathBuf,
    object_root: PathBuf,
    work_root: PathBuf,
    max_upload_bytes: usize,
    postgres_connection: String,
) -> Router {
    let job_store = JobStore::new();
    let schema_catalog = load_schema_catalog(&schema_path);
    let state = Arc::new(AppState {
        schema_path,
        schema_catalog,
        object_root,
        work_root,
        max_upload_bytes,
        max_prefix_fanout: 10_000,
        max_prefix_parallelism: DEFAULT_MAX_PREFIX_PARALLELISM,
        zip_max_total_bytes: DEFAULT_ZIP_MAX_TOTAL_BYTES,
        zip_max_entries: DEFAULT_ZIP_MAX_ENTRIES,
        persist_raw_text: false,
        persist_raw_fields: false,
        request_timeout: Duration::from_secs(120),
        rate_limiter: Arc::new(make_rate_limiter(
            NonZeroU32::new(5).expect("non-zero rps"),
            NonZeroU32::new(20).expect("non-zero burst"),
        )),
        idempotency: Arc::new(default_idempotency_store()),
        system_of_record: SystemOfRecordConfig::Postgres {
            connection: postgres_connection.clone(),
            schema: "swiftpipe".to_string(),
        },
        ready_check: Some(ReadyCheck::postgres(postgres_connection)),
        job_store,
        job_queue: None,
        metrics: Arc::new(Metrics::default()),
    });
    make_router(state, None)
}

pub fn make_full_queue_router_for_test(
    schema_path: PathBuf,
    object_root: PathBuf,
    work_root: PathBuf,
    max_upload_bytes: usize,
) -> Router {
    let job_store = JobStore::new();
    let metrics = Arc::new(Metrics::default());
    let (sender, receiver) = tokio::sync::mpsc::channel(1);
    sender
        .try_send(JobTask::Upload {
            job_id: "seeded-full-queue-job".to_string(),
            body: Vec::new(),
            message_type: None,
            outputs: None,
        })
        .expect("seed capacity-one queue");
    let _receiver = Box::leak(Box::new(receiver));
    let queue = JobQueue::from_sender_for_test(sender, Arc::clone(&metrics));
    let schema_catalog = load_schema_catalog(&schema_path);
    let state = Arc::new(AppState {
        schema_path,
        schema_catalog,
        object_root,
        work_root,
        max_upload_bytes,
        max_prefix_fanout: 10_000,
        max_prefix_parallelism: DEFAULT_MAX_PREFIX_PARALLELISM,
        zip_max_total_bytes: DEFAULT_ZIP_MAX_TOTAL_BYTES,
        zip_max_entries: DEFAULT_ZIP_MAX_ENTRIES,
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
        job_store,
        job_queue: Some(queue),
        metrics,
    });
    make_router(state, None)
}

fn test_status(status: TestJobStatus) -> job_store::JobStatus {
    match status {
        TestJobStatus::Queued => job_store::JobStatus::Queued,
        TestJobStatus::Failed { error } => job_store::JobStatus::Failed { error },
        TestJobStatus::Completed { manifest } => job_store::JobStatus::Completed { manifest },
    }
}
