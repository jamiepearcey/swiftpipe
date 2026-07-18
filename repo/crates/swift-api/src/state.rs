use std::num::{NonZeroU32, NonZeroUsize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use governor::{DefaultKeyedRateLimiter, Quota, RateLimiter};
use lru::LruCache;
use prometheus::{
    Histogram, HistogramOpts, IntCounter, IntCounterVec, IntGauge, IntGaugeVec, Opts, Registry,
};

use crate::job_store::JobStore;
use crate::queue::JobQueue;
use crate::system_record::{ReadyCheck, SystemOfRecordConfig};
use swift_schema::SchemaCatalog;

// ---------------------------------------------------------------------------
// Metrics
// ---------------------------------------------------------------------------

/// Metrics exposed at `GET /metrics` and `GET /metrics/json`.
#[derive(Debug)]
pub(crate) struct Metrics {
    pub(crate) registry: Registry,
    pub(crate) jobs_submitted: IntCounter,
    pub(crate) jobs_completed: IntCounter,
    pub(crate) jobs_failed: IntCounter,
    pub(crate) messages_processed: IntCounterVec,
    pub(crate) parquet_bytes_written: IntCounter,
    pub(crate) duckdb_rows_written: IntCounter,
    pub(crate) object_store_ops: IntCounterVec,
    pub(crate) api_errors: IntCounterVec,
    pub(crate) jobs_by_status: IntGaugeVec,
    pub(crate) upload_body_bytes: Histogram,
    pub(crate) prefix_job_fanout_objects: Histogram,
    pub(crate) jobs_in_flight: IntGauge,
    pub(crate) queue_depth: IntGauge,
    pub(crate) job_duration_seconds: Histogram,
}

impl Default for Metrics {
    fn default() -> Self {
        let registry = Registry::new();
        let jobs_submitted = register_counter(
            &registry,
            "swiftpipe_jobs_submitted_total",
            "Total jobs submitted since startup",
        );
        let jobs_completed = register_counter(
            &registry,
            "swiftpipe_jobs_completed_total",
            "Total jobs completed successfully since startup",
        );
        let jobs_failed = register_counter(
            &registry,
            "swiftpipe_jobs_failed_total",
            "Total jobs failed since startup",
        );
        let messages_processed = IntCounterVec::new(
            Opts::new(
                "swiftpipe_messages_processed_total",
                "Total messages processed since startup by SWIFT message type",
            ),
            &["message_type"],
        )
        .expect("valid messages processed counter opts");
        registry
            .register(Box::new(messages_processed.clone()))
            .expect("unique messages processed counter");
        let parquet_bytes_written = register_counter(
            &registry,
            "swiftpipe_parquet_bytes_written_total",
            "Total Parquet bytes written since startup",
        );
        let duckdb_rows_written = register_counter(
            &registry,
            "swiftpipe_duckdb_rows_written_total",
            "Total DuckDB rows written since startup",
        );
        let object_store_ops = IntCounterVec::new(
            Opts::new(
                "swiftpipe_object_store_ops_total",
                "Total successful object-store operations since startup by operation",
            ),
            &["op"],
        )
        .expect("valid object store ops counter opts");
        registry
            .register(Box::new(object_store_ops.clone()))
            .expect("unique object store ops counter");
        let api_errors = IntCounterVec::new(
            Opts::new(
                "swiftpipe_api_errors_total",
                "Total typed API errors since startup by stable error code",
            ),
            &["code"],
        )
        .expect("valid api errors counter opts");
        registry
            .register(Box::new(api_errors.clone()))
            .expect("unique api errors counter");
        let jobs_by_status = IntGaugeVec::new(
            Opts::new("swiftpipe_jobs_total", "Current jobs by lifecycle status"),
            &["status"],
        )
        .expect("valid jobs by status gauge opts");
        registry
            .register(Box::new(jobs_by_status.clone()))
            .expect("unique jobs by status gauge");
        for status in ["queued", "running", "succeeded", "failed", "stuck"] {
            jobs_by_status.with_label_values(&[status]).set(0);
        }
        let upload_body_bytes = Histogram::with_opts(
            HistogramOpts::new(
                "swiftpipe_upload_body_bytes",
                "Upload request body size in bytes",
            )
            .buckets(vec![
                1024.0,
                4.0 * 1024.0,
                16.0 * 1024.0,
                64.0 * 1024.0,
                256.0 * 1024.0,
                1024.0 * 1024.0,
                4.0 * 1024.0 * 1024.0,
                16.0 * 1024.0 * 1024.0,
                64.0 * 1024.0 * 1024.0,
                256.0 * 1024.0 * 1024.0,
            ]),
        )
        .expect("valid upload body bytes histogram");
        registry
            .register(Box::new(upload_body_bytes.clone()))
            .expect("unique upload body bytes histogram");
        let prefix_job_fanout_objects = Histogram::with_opts(
            HistogramOpts::new(
                "swiftpipe_prefix_job_fanout_objects",
                "Object count matched by each accepted prefix job",
            )
            .buckets(vec![
                1.0, 2.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 5000.0, 10000.0,
            ]),
        )
        .expect("valid prefix job fanout histogram");
        registry
            .register(Box::new(prefix_job_fanout_objects.clone()))
            .expect("unique prefix job fanout histogram");
        let jobs_in_flight = register_gauge(
            &registry,
            "swiftpipe_jobs_in_flight",
            "Current number of jobs being processed",
        );
        let queue_depth = register_gauge(
            &registry,
            "swiftpipe_queue_depth",
            "Current number of queued tasks not yet picked up by a worker",
        );
        let job_duration_seconds = Histogram::with_opts(
            HistogramOpts::new(
                "swiftpipe_job_duration_seconds",
                "Total job processing duration in seconds",
            )
            .buckets(vec![
                0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0, 120.0,
            ]),
        )
        .expect("valid job duration histogram");
        registry
            .register(Box::new(job_duration_seconds.clone()))
            .expect("unique job duration histogram");
        Self {
            registry,
            jobs_submitted,
            jobs_completed,
            jobs_failed,
            messages_processed,
            parquet_bytes_written,
            duckdb_rows_written,
            object_store_ops,
            api_errors,
            jobs_by_status,
            upload_body_bytes,
            prefix_job_fanout_objects,
            jobs_in_flight,
            queue_depth,
            job_duration_seconds,
        }
    }
}

impl Metrics {
    pub(crate) fn submitted(&self) {
        self.jobs_submitted.inc();
        self.jobs_by_status.with_label_values(&["queued"]).inc();
        self.jobs_in_flight.inc();
    }

    pub(crate) fn completed(&self, duration: Duration) {
        self.jobs_completed.inc();
        self.jobs_by_status.with_label_values(&["running"]).dec();
        self.jobs_by_status.with_label_values(&["succeeded"]).inc();
        self.jobs_in_flight.dec();
        self.job_duration_seconds.observe(duration.as_secs_f64());
    }

    pub(crate) fn message_processed(&self, message_type: &str) {
        self.messages_processed
            .with_label_values(&[message_type])
            .inc();
    }

    pub(crate) fn parquet_bytes_written(&self, bytes: u64) {
        self.parquet_bytes_written.inc_by(bytes);
    }

    pub(crate) fn duckdb_rows_written(&self, rows: u64) {
        self.duckdb_rows_written.inc_by(rows);
    }

    pub(crate) fn object_store_op(&self, op: &str) {
        self.object_store_ops.with_label_values(&[op]).inc();
    }

    pub(crate) fn api_error(&self, code: &str) {
        self.api_errors.with_label_values(&[code]).inc();
    }

    pub(crate) fn upload_body_bytes(&self, bytes: usize) {
        let observed = u32::try_from(bytes).unwrap_or(u32::MAX);
        self.upload_body_bytes.observe(f64::from(observed));
    }

    pub(crate) fn prefix_job_fanout_objects(&self, count: usize) {
        let observed = u32::try_from(count).unwrap_or(u32::MAX);
        self.prefix_job_fanout_objects.observe(f64::from(observed));
    }

    pub(crate) fn failed(&self, duration: Option<Duration>) {
        self.jobs_failed.inc();
        if duration.is_some() {
            self.jobs_by_status.with_label_values(&["running"]).dec();
        } else {
            self.jobs_by_status.with_label_values(&["queued"]).dec();
        }
        self.jobs_by_status.with_label_values(&["failed"]).inc();
        self.jobs_in_flight.dec();
        if let Some(duration) = duration {
            self.job_duration_seconds.observe(duration.as_secs_f64());
        }
    }

    pub(crate) fn stuck(&self) {
        self.jobs_failed.inc();
        self.jobs_by_status.with_label_values(&["running"]).dec();
        self.jobs_by_status.with_label_values(&["stuck"]).inc();
        self.jobs_in_flight.dec();
    }

    pub(crate) fn set_queue_depth(&self, depth: usize) {
        self.queue_depth.set(depth as i64);
    }

    pub(crate) fn dispatched(&self) {
        self.queue_depth.dec();
        self.jobs_by_status.with_label_values(&["queued"]).dec();
        self.jobs_by_status.with_label_values(&["running"]).inc();
    }
}

fn register_counter(registry: &Registry, name: &str, help: &str) -> IntCounter {
    let counter = IntCounter::with_opts(Opts::new(name, help)).expect("valid counter opts");
    registry
        .register(Box::new(counter.clone()))
        .expect("unique counter metric");
    counter
}

fn register_gauge(registry: &Registry, name: &str, help: &str) -> IntGauge {
    let gauge = IntGauge::with_opts(Opts::new(name, help)).expect("valid gauge opts");
    registry
        .register(Box::new(gauge.clone()))
        .expect("unique gauge metric");
    gauge
}

// ---------------------------------------------------------------------------
// AppState
// ---------------------------------------------------------------------------

pub(crate) const DEFAULT_ZIP_MAX_TOTAL_BYTES: u64 = 10 * 1024 * 1024 * 1024;
pub(crate) const DEFAULT_ZIP_MAX_ENTRIES: usize = 100_000;
pub(crate) const DEFAULT_MAX_PREFIX_PARALLELISM: usize = 4;

pub(crate) struct AppState {
    pub(crate) schema_path: PathBuf,
    pub(crate) schema_catalog: Arc<SchemaCatalog>,
    pub(crate) object_root: PathBuf,
    pub(crate) work_root: PathBuf,
    pub(crate) max_upload_bytes: usize,
    pub(crate) max_prefix_fanout: usize,
    pub(crate) max_prefix_parallelism: usize,
    pub(crate) zip_max_total_bytes: u64,
    pub(crate) zip_max_entries: usize,
    pub(crate) persist_raw_text: bool,
    pub(crate) persist_raw_fields: bool,
    pub(crate) request_timeout: Duration,
    pub(crate) rate_limiter: Arc<DefaultKeyedRateLimiter<String>>,
    pub(crate) idempotency: Arc<IdempotencyStore>,
    pub(crate) system_of_record: SystemOfRecordConfig,
    pub(crate) ready_check: Option<ReadyCheck>,
    /// Shared in-memory job status store.
    pub(crate) job_store: Arc<JobStore>,
    /// Sender side of the worker queue; `None` only in unit tests.
    pub(crate) job_queue: Option<JobQueue>,
    /// Atomic metrics counters.
    pub(crate) metrics: Arc<Metrics>,
}

#[cfg(test)]
pub(crate) fn empty_schema_catalog() -> Arc<SchemaCatalog> {
    Arc::new(SchemaCatalog {
        field_types: Vec::new(),
        messages: Vec::new(),
    })
}

pub(crate) fn make_rate_limiter(
    rps: NonZeroU32,
    burst: NonZeroU32,
) -> DefaultKeyedRateLimiter<String> {
    RateLimiter::keyed(Quota::per_second(rps).allow_burst(burst))
}

pub(crate) struct IdempotencyStore {
    ttl: Duration,
    entries: Mutex<LruCache<String, IdempotencyEntry>>,
}

impl IdempotencyStore {
    pub(crate) fn new(capacity: NonZeroUsize, ttl: Duration) -> Self {
        Self {
            ttl,
            entries: Mutex::new(LruCache::new(capacity)),
        }
    }

    pub(crate) fn get(&self, key: &str) -> Option<String> {
        let mut entries = self.entries.lock().expect("idempotency store poisoned");
        let entry = entries.get(key)?;
        if entry.inserted_at.elapsed() <= self.ttl {
            return Some(entry.job_id.clone());
        }
        entries.pop(key);
        None
    }

    pub(crate) fn insert(&self, key: String, job_id: String) {
        let mut entries = self.entries.lock().expect("idempotency store poisoned");
        entries.put(
            key,
            IdempotencyEntry {
                job_id,
                inserted_at: Instant::now(),
            },
        );
    }
}

struct IdempotencyEntry {
    job_id: String,
    inserted_at: Instant,
}

pub(crate) fn default_idempotency_store() -> IdempotencyStore {
    IdempotencyStore::new(
        NonZeroUsize::new(10_000).expect("non-zero idempotency capacity"),
        Duration::from_secs(24 * 60 * 60),
    )
}
