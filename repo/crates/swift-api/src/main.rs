#![forbid(unsafe_code)]
#![deny(warnings, rust_2018_idioms, missing_debug_implementations)]
#![warn(clippy::pedantic)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
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

use anyhow::{Context, Result};
use clap::Parser;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;

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
use object_store::gc_outbox_jobs;
use queue::{spawn_reaper, JobQueue};
use routes::make_router;
use state::{
    default_idempotency_store, make_rate_limiter, AppState, Metrics,
    DEFAULT_MAX_PREFIX_PARALLELISM, DEFAULT_ZIP_MAX_ENTRIES, DEFAULT_ZIP_MAX_TOTAL_BYTES,
};
use system_record::{ReadyCheck, SystemOfRecordConfig};

const DEFAULT_RUST_LOG: &str = "swiftpipe_api=info,tower_http=warn";
const LOG_FORMAT_ENV: &str = "SWIFTPIPE_LOG_FORMAT";
const AUTH_REQUIRED_ENV: &str = "SWIFTPIPE_AUTH_REQUIRED";
const CONFIG_ERROR_EXIT_CODE: i32 = 78;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogFormat {
    Text,
    Json,
}

// ---------------------------------------------------------------------------
// CLI args — secrets prefer env vars over flags
// ---------------------------------------------------------------------------

#[derive(Debug, Parser)]
#[command(name = "swiftpipe-api")]
#[command(about = "Self-hosted SwiftPipe artifact API")]
struct Args {
    #[arg(long, default_value = "127.0.0.1:8080")]
    listen: String,
    #[arg(long, default_value = "examples/schemas")]
    schema_path: PathBuf,
    #[arg(long, default_value = ".swiftpipe-objects")]
    object_root: PathBuf,
    #[arg(long, default_value = ".swiftpipe-work")]
    work_root: PathBuf,
    #[arg(long, default_value_t = 100 * 1024 * 1024)]
    max_upload_bytes: usize,
    /// Maximum number of objects a single prefix job may expand to.
    #[arg(long, default_value_t = 10_000)]
    max_prefix_fanout: usize,
    /// Maximum worker threads used to prepare objects within one prefix job.
    #[arg(long, default_value_t = DEFAULT_MAX_PREFIX_PARALLELISM)]
    max_prefix_parallelism: usize,
    /// Maximum bytes allowed in a generated job export zip.
    #[arg(long, default_value_t = DEFAULT_ZIP_MAX_TOTAL_BYTES)]
    zip_max_total_bytes: u64,
    /// Maximum entries allowed in a generated job export zip.
    #[arg(long, default_value_t = DEFAULT_ZIP_MAX_ENTRIES)]
    zip_max_entries: usize,
    #[arg(long)]
    persist_raw_text: bool,
    #[arg(long)]
    persist_raw_fields: bool,
    #[arg(long, default_value = "none")]
    system_of_record: String,
    #[arg(long)]
    system_record_file: Option<PathBuf>,
    /// Postgres connection string. Prefer the `SWIFTPIPE_POSTGRES_CONNECTION`
    /// env var; this flag is a fallback for local development.
    #[arg(long)]
    postgres_connection: Option<String>,
    #[arg(long, default_value = "swiftpipe")]
    system_record_schema: String,
    /// SQL Server connection string. Prefer `SWIFTPIPE_SQLSERVER_CONNECTION`.
    #[arg(long)]
    sqlserver_connection: Option<String>,
    /// Worker pool size for async job processing.
    #[arg(long, default_value_t = 4)]
    job_workers: usize,
    /// Bounded job queue capacity (HTTP 503 when full).
    #[arg(long, default_value_t = 256)]
    job_queue_capacity: usize,
    /// Seconds before a running job is considered stuck and marked failed.
    #[arg(long, default_value_t = 1800)]
    job_stuck_timeout_secs: u64,
    /// Per-request timeout for `/v1/*` handlers in seconds.
    /// `SWIFTPIPE_REQUEST_TIMEOUT_SECS` takes precedence when set.
    #[arg(long, default_value_t = 120)]
    request_timeout_secs: u64,
    /// Per-IP sustained rate limit for write endpoints.
    #[arg(long, default_value_t = 5)]
    rate_limit_rps: u32,
    /// Per-IP burst capacity for write endpoints.
    #[arg(long, default_value_t = 20)]
    rate_limit_burst: u32,
    /// Path to the compiled Vite UI dist directory. Served at /app/* when set.
    #[arg(long)]
    ui_dist: Option<PathBuf>,
    /// Refuse startup unless `SWIFTPIPE_AUTH_TOKEN` is configured.
    #[arg(long)]
    auth_required: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let args = Args::parse();
    enforce_auth_required(&args);
    std::fs::create_dir_all(&args.object_root)?;
    std::fs::create_dir_all(&args.work_root)?;
    let system_of_record = system_of_record_config(&args)?;
    let ready_check = ReadyCheck::from_config(&system_of_record);
    let request_timeout = request_timeout(&args)?;
    let rate_limiter = Arc::new(make_rate_limiter(
        nonzero_arg(args.rate_limit_rps, "--rate-limit-rps")?,
        nonzero_arg(args.rate_limit_burst, "--rate-limit-burst")?,
    ));

    let job_store = JobStore::new();
    let metrics = Arc::new(Metrics::default());
    let schema_catalog = Arc::new(job::load_catalog(std::slice::from_ref(&args.schema_path))?);
    schema_catalog.validate_rendering()?;

    // Worker state intentionally has no queue handle: workers process already-submitted tasks.
    let worker_state = Arc::new(AppState {
        schema_path: args.schema_path.clone(),
        schema_catalog: Arc::clone(&schema_catalog),
        object_root: args.object_root.clone(),
        work_root: args.work_root.clone(),
        max_upload_bytes: args.max_upload_bytes,
        max_prefix_fanout: args.max_prefix_fanout,
        max_prefix_parallelism: args.max_prefix_parallelism,
        zip_max_total_bytes: args.zip_max_total_bytes,
        zip_max_entries: args.zip_max_entries,
        persist_raw_text: args.persist_raw_text,
        persist_raw_fields: args.persist_raw_fields,
        request_timeout,
        rate_limiter: Arc::clone(&rate_limiter),
        idempotency: Arc::new(default_idempotency_store()),
        system_of_record: system_of_record.clone(),
        ready_check: ready_check.clone(),
        job_store: Arc::clone(&job_store),
        job_queue: None,
        metrics: Arc::clone(&metrics),
    });

    // Spawn worker pool and reaper, then wire the queue sender into serving state.
    let (queue, mut worker_join_set) = JobQueue::spawn(
        args.job_workers,
        args.job_queue_capacity,
        Arc::clone(&worker_state),
        Arc::clone(&job_store),
    );
    let state = Arc::new(AppState {
        schema_path: args.schema_path,
        schema_catalog,
        object_root: args.object_root,
        work_root: args.work_root,
        max_upload_bytes: args.max_upload_bytes,
        max_prefix_fanout: args.max_prefix_fanout,
        max_prefix_parallelism: args.max_prefix_parallelism,
        zip_max_total_bytes: args.zip_max_total_bytes,
        zip_max_entries: args.zip_max_entries,
        persist_raw_text: args.persist_raw_text,
        persist_raw_fields: args.persist_raw_fields,
        request_timeout,
        rate_limiter,
        idempotency: Arc::new(default_idempotency_store()),
        system_of_record,
        ready_check,
        job_store: Arc::clone(&job_store),
        job_queue: Some(queue),
        metrics: Arc::clone(&metrics),
    });

    let stuck_timeout = Duration::from_secs(args.job_stuck_timeout_secs);
    let _reaper = spawn_reaper(
        Arc::clone(&job_store),
        Arc::clone(&metrics),
        Duration::from_secs(60),
        stuck_timeout,
    );

    // GC: remove outbox job directories older than 7 days every hour.
    let gc_object_root = state.object_root.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(3600));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            let root = gc_object_root.clone();
            let removed = tokio::task::spawn_blocking(move || {
                gc_outbox_jobs(&root, Duration::from_secs(7 * 24 * 3600))
            })
            .await
            .unwrap_or(0);
            if removed > 0 {
                tracing::info!(removed, "gc: removed old outbox job directories");
            }
        }
    });

    let app = make_router(Arc::clone(&state), args.ui_dist);
    let listener = TcpListener::bind(&args.listen)
        .await
        .with_context(|| format!("failed to bind {}", args.listen))?;
    tracing::info!(
        listen = %args.listen,
        workers = args.job_workers,
        queue_capacity = args.job_queue_capacity,
        "swiftpipe-api starting"
    );

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        if let Err(err) = shutdown_signal().await {
            tracing::error!(error = %err, "shutdown signal handler failed");
        }
    })
    .await?;

    // Wait for all workers to drain.
    while worker_join_set.join_next().await.is_some() {}
    tracing::info!("all workers stopped; exiting");

    Ok(())
}

fn init_tracing() {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| DEFAULT_RUST_LOG.into());
    match log_format_from_env() {
        LogFormat::Text => tracing_subscriber::fmt().with_env_filter(env_filter).init(),
        LogFormat::Json => tracing_subscriber::fmt()
            .json()
            .with_env_filter(env_filter)
            .init(),
    }
}

fn log_format_from_env() -> LogFormat {
    log_format(std::env::var(LOG_FORMAT_ENV).ok().as_deref())
}

fn log_format(value: Option<&str>) -> LogFormat {
    match value.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("json") => LogFormat::Json,
        _ => LogFormat::Text,
    }
}

fn enforce_auth_required(args: &Args) {
    if auth_required(args) && !auth::has_configured_auth_token_from_env() {
        tracing::error!(
            auth_token_env = auth::AUTH_TOKEN_ENV,
            auth_required_env = AUTH_REQUIRED_ENV,
            "auth is required but no bearer token is configured"
        );
        eprintln!(
            "configuration error: --auth-required/{AUTH_REQUIRED_ENV}=1 requires {}",
            auth::AUTH_TOKEN_ENV
        );
        std::process::exit(CONFIG_ERROR_EXIT_CODE);
    }
}

fn auth_required(args: &Args) -> bool {
    args.auth_required || env_flag_enabled(AUTH_REQUIRED_ENV)
}

fn env_flag_enabled(name: &str) -> bool {
    std::env::var(name).ok().is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn system_of_record_config(args: &Args) -> Result<SystemOfRecordConfig> {
    match args.system_of_record.as_str() {
        "none" => Ok(SystemOfRecordConfig::None),
        "file" => Ok(SystemOfRecordConfig::File {
            path: args
                .system_record_file
                .clone()
                .unwrap_or_else(|| args.work_root.join("system-record.jsonl")),
        }),
        "postgres" => {
            // Env var takes precedence over CLI flag.
            let connection = std::env::var("SWIFTPIPE_POSTGRES_CONNECTION")
                .ok()
                .filter(|s| !s.is_empty())
                .or_else(|| args.postgres_connection.clone())
                .context(
                    "postgres connection required: set SWIFTPIPE_POSTGRES_CONNECTION env var \
                     or pass --postgres-connection",
                )?;
            Ok(SystemOfRecordConfig::Postgres {
                connection,
                schema: args.system_record_schema.clone(),
            })
        }
        "sqlserver" => {
            let connection = std::env::var("SWIFTPIPE_SQLSERVER_CONNECTION")
                .ok()
                .filter(|s| !s.is_empty())
                .or_else(|| args.sqlserver_connection.clone())
                .context(
                    "sqlserver connection required: set SWIFTPIPE_SQLSERVER_CONNECTION env var \
                     or pass --sqlserver-connection",
                )?;
            Ok(SystemOfRecordConfig::SqlServer {
                connection,
                schema: args.system_record_schema.clone(),
            })
        }
        other => anyhow::bail!(
            "--system-of-record must be one of none, file, postgres, sqlserver; got {other}"
        ),
    }
}

fn request_timeout(args: &Args) -> Result<Duration> {
    let seconds = std::env::var("SWIFTPIPE_REQUEST_TIMEOUT_SECS")
        .ok()
        .filter(|value| !value.is_empty())
        .map_or_else(
            || Ok(args.request_timeout_secs),
            |value| {
                value
                    .parse::<u64>()
                    .context("SWIFTPIPE_REQUEST_TIMEOUT_SECS must be an integer number of seconds")
            },
        )?;
    Ok(Duration::from_secs(seconds))
}

fn nonzero_arg(value: u32, name: &str) -> Result<NonZeroU32> {
    NonZeroU32::new(value).with_context(|| format!("{name} must be greater than zero"))
}

async fn shutdown_signal() -> Result<()> {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .context("failed to install Ctrl+C handler")
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .context("failed to install SIGTERM handler")?
            .recv()
            .await;
        Ok::<(), anyhow::Error>(())
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<Result<()>>();

    tokio::select! {
        result = ctrl_c => {
            result?;
            tracing::info!("received ctrl-c, shutting down");
        }
        result = terminate => {
            result?;
            tracing::info!("received SIGTERM, shutting down");
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Integration tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::{
        new_job_id, process_job, process_job_request, process_upload, JobProcessRequest,
    };
    use crate::manifest::JobRequest;
    use crate::object_store::{LocalObjectStore, ObjectStore};
    use zip::ZipArchive;

    fn test_state(name: &str) -> (PathBuf, AppState) {
        let root = std::env::temp_dir().join(format!("{name}-{}", new_job_id()));
        let object_root = root.join("objects");
        let work_root = root.join("work");
        std::fs::create_dir_all(&object_root).expect("object root");
        std::fs::create_dir_all(&work_root).expect("work root");
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("workspace root")
            .to_path_buf();
        let schema_path = workspace_root.join("examples/schemas");
        let schema_catalog = Arc::new(
            crate::job::load_catalog(std::slice::from_ref(&schema_path))
                .expect("schema catalog loads"),
        );
        schema_catalog
            .validate_rendering()
            .expect("schema catalog validates rendering");
        (
            root,
            AppState {
                schema_path,
                schema_catalog,
                object_root,
                work_root,
                max_upload_bytes: 100 * 1024 * 1024,
                max_prefix_fanout: 10_000,
                max_prefix_parallelism: crate::state::DEFAULT_MAX_PREFIX_PARALLELISM,
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
                job_store: JobStore::new(),
                job_queue: None,
                metrics: Arc::new(Metrics::default()),
            },
        )
    }

    fn sample_fin(name: &str) -> Vec<u8> {
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("workspace root")
            .to_path_buf();
        std::fs::read(workspace_root.join(name)).expect("sample FIN")
    }

    #[test]
    fn default_rust_log_keeps_tower_http_at_warn() {
        assert_eq!(DEFAULT_RUST_LOG, "swiftpipe_api=info,tower_http=warn");
    }

    #[test]
    fn log_format_from_env_accepts_json_only() {
        assert_eq!(log_format(Some("json")), LogFormat::Json);
        assert_eq!(log_format(Some(" JSON ")), LogFormat::Json);
        assert_eq!(log_format(Some("text")), LogFormat::Text);
        assert_eq!(log_format(None), LogFormat::Text);
    }

    #[test]
    fn job_request_rejects_non_s3_output_prefix_before_object_store_access() {
        let (_root, state) = test_state("swiftpipe-api-output-prefix-scheme");
        let error = process_job_request(
            &state,
            "job-output-scheme".to_string(),
            JobRequest {
                input_uri: Some("s3://swiftpipe-inbox/input.fin".to_string()),
                input_prefix: None,
                output_prefix: Some("file:///tmp/out/".to_string()),
                include_suffix: None,
                message_type: None,
                render_validate: None,
                outputs: None,
            },
        )
        .expect_err("non-s3 output_prefix should be rejected");

        assert_eq!(error.api_code(), crate::error::ApiErrorCode::BadRequest);
        assert!(error.to_string().contains("expected s3://"));
    }

    #[test]
    fn process_upload_writes_manifest_parquet_and_rendered_fin() {
        let (root, state) = test_state("swiftpipe-api-e2e");
        let body = sample_fin("examples/mt540_sample.fin");

        let manifest = process_upload(&state, new_job_id(), body, Some("MT540".to_string()), None)
            .expect("processes upload");

        let store = LocalObjectStore::new(&state.object_root);
        assert_eq!(manifest.status, "completed");
        assert_eq!(manifest.counts.messages, 1);
        assert_eq!(manifest.counts.rendered, 1);
        assert_eq!(manifest.contract_version, "1");
        assert!(store
            .local_path_for_test(&manifest.outputs.manifest)
            .expect("manifest path")
            .is_file());
        assert!(store
            .local_path_for_test(&manifest.outputs.normalized_parquet_prefix)
            .expect("parquet path")
            .join("swift_raw_messages.parquet")
            .is_file());

        let raw_messages_path = store
            .local_path_for_test(&manifest.outputs.normalized_parquet_prefix)
            .expect("parquet path")
            .join("swift_raw_messages.parquet");
        let conn = duckdb::Connection::open_in_memory().expect("opens duckdb");
        let raw_text: String = conn
            .query_row(
                "SELECT raw_text FROM read_parquet(?)",
                [raw_messages_path.to_string_lossy().as_ref()],
                |row| row.get(0),
            )
            .expect("reads raw text parquet");
        assert_eq!(raw_text, "");

        let raw_fields_path = store
            .local_path_for_test(&manifest.outputs.normalized_parquet_prefix)
            .expect("parquet path")
            .join("swift_fields.parquet");
        let raw_value: String = conn
            .query_row(
                "SELECT raw_value FROM read_parquet(?) ORDER BY tag LIMIT 1",
                [raw_fields_path.to_string_lossy().as_ref()],
                |row| row.get(0),
            )
            .expect("reads raw field parquet");
        assert_eq!(raw_value, "");

        let rendered_uri = manifest.messages[0]
            .rendered_uri
            .as_ref()
            .expect("rendered uri");
        let rendered = String::from_utf8(store.get(rendered_uri).expect("rendered FIN"))
            .expect("rendered FIN utf8");
        assert!(rendered.contains("{2:I540BANKDEFFXXXXN}"));
        assert!(rendered.contains(":20C::SEME//"));

        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn prefix_job_snapshots_inputs_and_processes_each_match() {
        let (root, state) = test_state("swiftpipe-api-prefix");
        let store = LocalObjectStore::new(&state.object_root);
        store
            .put_atomic(
                "s3://swiftpipe-inbox/daily/a.fin",
                &sample_fin("examples/mt540_sample.fin"),
            )
            .expect("seed a");
        store
            .put_atomic(
                "s3://swiftpipe-inbox/daily/b.fin",
                &sample_fin("examples/mt540_sample.fin"),
            )
            .expect("seed b");
        store
            .put_atomic("s3://swiftpipe-inbox/daily/skip.txt", b"skip")
            .expect("seed skip");

        let manifest = process_job_request(
            &state,
            "prefix-test".to_string(),
            JobRequest {
                input_uri: None,
                input_prefix: Some("s3://swiftpipe-inbox/daily/".to_string()),
                output_prefix: Some("s3://swiftpipe-outbox/jobs/prefix-test/".to_string()),
                message_type: Some("MT540".to_string()),
                include_suffix: Some(".fin".to_string()),
                render_validate: Some(true),
                outputs: None,
            },
        )
        .expect("process prefix");

        assert_eq!(manifest.status, "completed");
        assert_eq!(manifest.input_uris.len(), 2);
        assert_eq!(manifest.counts.input_objects, 2);
        assert_eq!(manifest.counts.messages, 2);

        let snapshot: Vec<String> = serde_json::from_slice(
            &store
                .get("s3://swiftpipe-outbox/jobs/prefix-test/input_snapshot.json")
                .expect("snapshot"),
        )
        .expect("snapshot json");
        assert_eq!(snapshot, manifest.input_uris);

        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn rendered_only_job_bypasses_duckdb_artifacts() {
        let (root, state) = test_state("swiftpipe-api-rendered-only");
        let store = LocalObjectStore::new(&state.object_root);
        store
            .put_atomic(
                "s3://swiftpipe-inbox/rendered-only/a.fin",
                &sample_fin("examples/mt540_sample.fin"),
            )
            .expect("seed input");

        let manifest = process_job_request(
            &state,
            "rendered-only".to_string(),
            JobRequest {
                input_uri: Some("s3://swiftpipe-inbox/rendered-only/a.fin".to_string()),
                input_prefix: None,
                output_prefix: Some("s3://swiftpipe-outbox/jobs/rendered-only/".to_string()),
                message_type: Some("MT540".to_string()),
                include_suffix: None,
                render_validate: Some(true),
                outputs: Some(vec!["rendered".to_string()]),
            },
        )
        .expect("process rendered-only job");

        assert_eq!(manifest.status, "completed");
        assert_eq!(manifest.counts.messages, 1);
        assert_eq!(manifest.counts.rendered, 1);
        assert_eq!(manifest.timings.hydrate_write_ms, 0);
        assert_eq!(manifest.timings.zip_ms, 0);
        assert!(store
            .get("s3://swiftpipe-outbox/jobs/rendered-only/rendered/msg-1.fin")
            .is_ok());

        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn job_syncs_to_file_system_of_record() {
        let (root, mut state) = test_state("swiftpipe-api-system-record");
        let system_record_path = root.join("system-record.jsonl");
        state.system_of_record = SystemOfRecordConfig::File {
            path: system_record_path.clone(),
        };
        let store = LocalObjectStore::new(&state.object_root);
        store
            .put_atomic(
                "s3://swiftpipe-inbox/system-record/a.fin",
                &sample_fin("examples/mt540_sample.fin"),
            )
            .expect("seed input");

        let manifest = process_job_request(
            &state,
            "system-record".to_string(),
            JobRequest {
                input_uri: Some("s3://swiftpipe-inbox/system-record/a.fin".to_string()),
                input_prefix: None,
                output_prefix: Some("s3://swiftpipe-outbox/jobs/system-record/".to_string()),
                message_type: Some("MT540".to_string()),
                include_suffix: None,
                render_validate: Some(true),
                outputs: Some(vec!["rendered".to_string()]),
            },
        )
        .expect("process job");

        assert_eq!(manifest.status, "completed");
        let content = std::fs::read_to_string(system_record_path).expect("system record jsonl");
        assert!(content.contains("\"kind\":\"job\""));
        assert!(content.contains("\"status\":\"running\""));
        assert!(content.contains("\"status\":\"completed\""));

        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn prefix_job_records_failed_input_without_aborting_batch() {
        let (root, state) = test_state("swiftpipe-api-prefix-failure");
        let store = LocalObjectStore::new(&state.object_root);
        store
            .put_atomic(
                "s3://swiftpipe-inbox/mixed/a.fin",
                &sample_fin("examples/mt540_sample.fin"),
            )
            .expect("seed good");
        store
            .put_atomic("s3://swiftpipe-inbox/mixed/b.fin", b"not a FIN message")
            .expect("seed bad");

        let manifest = process_job_request(
            &state,
            "failure-test".to_string(),
            JobRequest {
                input_uri: None,
                input_prefix: Some("s3://swiftpipe-inbox/mixed/".to_string()),
                output_prefix: Some("s3://swiftpipe-outbox/jobs/failure-test/".to_string()),
                message_type: None,
                include_suffix: Some(".fin".to_string()),
                render_validate: Some(true),
                outputs: None,
            },
        )
        .expect("process mixed prefix");

        assert_eq!(manifest.status, "completed_with_errors");
        assert!(manifest.messages.iter().any(|m| m.status == "completed"));
        assert!(manifest.messages.iter().any(|m| m.status == "failed"));

        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn job_level_failure_writes_failed_manifest_and_status() {
        let (root, mut state) = test_state("swiftpipe-api-job-failure");
        let work_root_file = root.join("work-root-file");
        std::fs::write(&work_root_file, b"not a directory").expect("work root file");
        state.work_root = work_root_file;
        let store = LocalObjectStore::new(&state.object_root);
        store
            .put_atomic(
                "s3://swiftpipe-inbox/fatal/a.fin",
                &sample_fin("examples/mt540_sample.fin"),
            )
            .expect("seed input");

        let result = process_job(
            &state,
            &store,
            JobProcessRequest {
                job_id: "fatal-test".to_string(),
                input_uris: vec!["s3://swiftpipe-inbox/fatal/a.fin".to_string()],
                output_prefix: "s3://swiftpipe-outbox/jobs/fatal-test/".to_string(),
                configured_message_type: Some("MT540".to_string()),
                render_validate: true,
                output_selection: crate::job::OutputSelection::all(),
            },
        );

        assert!(result.is_err());
        let manifest: serde_json::Value = serde_json::from_slice(
            &store
                .get("s3://swiftpipe-outbox/jobs/fatal-test/manifest.json")
                .expect("manifest"),
        )
        .expect("manifest json");
        assert_eq!(manifest["status"], "failed");

        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn zip_contains_manifest_errors_rendered_fin_and_parquet() {
        let (root, state) = test_state("swiftpipe-api-zip");
        let manifest = process_upload(
            &state,
            new_job_id(),
            sample_fin("examples/mt540_sample.fin"),
            Some("MT540".to_string()),
            None,
        )
        .expect("process upload");
        let store = LocalObjectStore::new(&state.object_root);
        let zip_path = store
            .local_path_for_test(&manifest.outputs.zip)
            .expect("zip path");
        let file = std::fs::File::open(zip_path).expect("zip file");
        let mut zip = ZipArchive::new(file).expect("zip archive");
        let names = (0..zip.len())
            .map(|i| zip.by_index(i).expect("zip entry").name().to_string())
            .collect::<Vec<_>>();

        assert!(names.iter().any(|n| n == "manifest.json"));
        assert!(names.iter().any(|n| n == "errors.ndjson"));
        assert!(names.iter().any(|n| n == "rendered/msg-1.fin"));
        assert!(names
            .iter()
            .any(|n| n == "normalized/swift_raw_messages.parquet"));
        assert!(!names.iter().any(|n| n == "exports.zip"));

        std::fs::remove_dir_all(root).expect("cleanup");
    }
}
