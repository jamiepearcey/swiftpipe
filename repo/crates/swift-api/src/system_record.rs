use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::fs;
use std::future::Future;
use std::io::Write;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

type ReadyCheckFuture = Pin<Box<dyn Future<Output = Result<(), String>> + Send>>;

#[derive(Clone)]
pub(crate) struct ReadyCheck {
    name: &'static str,
    probe: Arc<dyn Fn() -> ReadyCheckFuture + Send + Sync>,
}

impl std::fmt::Debug for ReadyCheck {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ReadyCheck")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl ReadyCheck {
    pub(crate) fn postgres(connection: String) -> Self {
        let pool = Arc::new(
            sqlx::postgres::PgPoolOptions::new()
                .max_connections(1)
                .connect_lazy(&connection)
                .map_err(|err| format!("postgres pool error: {err}")),
        );
        Self {
            name: "postgres",
            probe: Arc::new(move || {
                let pool = Arc::clone(&pool);
                Box::pin(async move {
                    let pool = pool.as_ref().as_ref().map_err(Clone::clone)?;
                    sqlx::query("SELECT 1")
                        .execute(pool)
                        .await
                        .map(|_| ())
                        .map_err(|err| format!("postgres readiness probe failed: {err}"))
                })
            }),
        }
    }

    pub(crate) fn from_config(config: &SystemOfRecordConfig) -> Option<Self> {
        match config {
            SystemOfRecordConfig::Postgres { connection, .. } => {
                Some(Self::postgres(connection.clone()))
            }
            SystemOfRecordConfig::None
            | SystemOfRecordConfig::File { .. }
            | SystemOfRecordConfig::SqlServer { .. } => None,
        }
    }

    pub(crate) fn name(&self) -> &'static str {
        self.name
    }

    pub(crate) async fn check(&self) -> Result<(), String> {
        (self.probe)().await
    }
}

#[derive(Debug, Clone)]
pub(crate) enum SystemOfRecordConfig {
    None,
    File { path: PathBuf },
    Postgres { connection: String, schema: String },
    SqlServer { connection: String, schema: String },
}

pub(crate) trait SystemOfRecordSink {
    fn upsert_job(&mut self, job: &JobRecord) -> Result<()>;
    fn append_audit_event(&mut self, event: &AuditEvent) -> Result<()>;
    fn mark_artifact_committed(&mut self, artifact: &ArtifactRecord) -> Result<()>;
}

#[derive(Debug, Serialize)]
pub(crate) struct JobRecord {
    pub(crate) job_id: String,
    pub(crate) status: String,
    pub(crate) input_objects: usize,
    pub(crate) messages: usize,
    pub(crate) parse_errors: usize,
    pub(crate) rendered: usize,
    pub(crate) output_prefix: String,
    pub(crate) processing_elapsed_ms: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct AuditEvent {
    pub(crate) job_id: String,
    pub(crate) event_type: String,
    pub(crate) message: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ArtifactRecord {
    pub(crate) job_id: String,
    pub(crate) artifact_type: String,
    pub(crate) uri: String,
}

pub(crate) fn open_system_of_record(
    config: &SystemOfRecordConfig,
) -> Result<Box<dyn SystemOfRecordSink>> {
    match config {
        SystemOfRecordConfig::None => Ok(Box::new(NullSystemOfRecordSink)),
        SystemOfRecordConfig::File { path } => {
            Ok(Box::new(FileSystemOfRecordSink { path: path.clone() }))
        }
        SystemOfRecordConfig::Postgres { connection, schema } => {
            Ok(Box::new(SqlxPostgresSink::connect(connection, schema)?))
        }
        SystemOfRecordConfig::SqlServer { connection, schema } => Ok(Box::new(
            SqlServerSystemOfRecordSink::connect(connection, schema)?,
        )),
    }
}

// ---------------------------------------------------------------------------
// Null sink
// ---------------------------------------------------------------------------

struct NullSystemOfRecordSink;

impl SystemOfRecordSink for NullSystemOfRecordSink {
    fn upsert_job(&mut self, _job: &JobRecord) -> Result<()> {
        Ok(())
    }

    fn append_audit_event(&mut self, _event: &AuditEvent) -> Result<()> {
        Ok(())
    }

    fn mark_artifact_committed(&mut self, _artifact: &ArtifactRecord) -> Result<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// File sink
// ---------------------------------------------------------------------------

struct FileSystemOfRecordSink {
    path: PathBuf,
}

impl FileSystemOfRecordSink {
    fn append<T: Serialize>(&self, kind: &str, value: &T) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let line = serde_json::to_string(&serde_json::json!({
            "kind": kind,
            "record": value,
        }))?;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .with_context(|| format!("failed to open {}", self.path.display()))?;
        writeln!(file, "{line}")?;
        Ok(())
    }
}

impl SystemOfRecordSink for FileSystemOfRecordSink {
    fn upsert_job(&mut self, job: &JobRecord) -> Result<()> {
        self.append("job", job)
    }

    fn append_audit_event(&mut self, event: &AuditEvent) -> Result<()> {
        self.append("audit_event", event)
    }

    fn mark_artifact_committed(&mut self, artifact: &ArtifactRecord) -> Result<()> {
        self.append("artifact", artifact)
    }
}

// ---------------------------------------------------------------------------
// sqlx Postgres sink
//
// The trait methods are synchronous (called from `spawn_blocking`). We drive
// the async pool via `Handle::current().block_on(...)` which is valid from a
// non-async thread (spawn_blocking runs on a dedicated thread pool, not on a
// tokio worker).
// ---------------------------------------------------------------------------

struct SqlxPostgresSink {
    pool: sqlx::PgPool,
    schema: String,
}

impl SqlxPostgresSink {
    fn connect(connection: &str, schema: &str) -> Result<Self> {
        let handle = tokio::runtime::Handle::try_current()
            .context("SqlxPostgresSink requires a tokio runtime")?;
        let pool = handle
            .block_on(async {
                sqlx::postgres::PgPoolOptions::new()
                    .max_connections(4)
                    .connect(connection)
                    .await
            })
            .context("failed to connect to Postgres")?;

        let sink = Self {
            pool,
            schema: schema.to_string(),
        };
        sink.ensure_schema()?;
        Ok(sink)
    }

    fn ensure_schema(&self) -> Result<()> {
        let handle = tokio::runtime::Handle::try_current()
            .context("SqlxPostgresSink requires a tokio runtime")?;
        handle.block_on(self.run_migrations())?;
        Ok(())
    }

    async fn run_migrations(&self) -> Result<()> {
        // Use a quoted identifier to avoid injection; schema name is validated
        // at config time (comes from the CLI/env, not user input).
        let schema = &self.schema;
        sqlx::query(&format!("CREATE SCHEMA IF NOT EXISTS \"{schema}\""))
            .execute(&self.pool)
            .await
            .context("failed to create schema")?;

        sqlx::query(&format!(
            r#"CREATE TABLE IF NOT EXISTS "{schema}".swiftpipe_jobs (
                job_id TEXT PRIMARY KEY,
                status TEXT NOT NULL,
                input_objects BIGINT NOT NULL DEFAULT 0,
                messages BIGINT NOT NULL DEFAULT 0,
                parse_errors BIGINT NOT NULL DEFAULT 0,
                rendered BIGINT NOT NULL DEFAULT 0,
                output_prefix TEXT NOT NULL DEFAULT '',
                processing_elapsed_ms BIGINT NOT NULL DEFAULT 0,
                updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
            )"#
        ))
        .execute(&self.pool)
        .await
        .context("failed to create swiftpipe_jobs")?;

        sqlx::query(&format!(
            r#"CREATE TABLE IF NOT EXISTS "{schema}".swiftpipe_audit_events (
                id BIGSERIAL PRIMARY KEY,
                job_id TEXT NOT NULL,
                event_type TEXT NOT NULL,
                message TEXT NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT now()
            )"#
        ))
        .execute(&self.pool)
        .await
        .context("failed to create swiftpipe_audit_events")?;

        sqlx::query(&format!(
            r#"CREATE TABLE IF NOT EXISTS "{schema}".swiftpipe_artifacts (
                job_id TEXT NOT NULL,
                artifact_type TEXT NOT NULL,
                uri TEXT NOT NULL,
                committed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
                PRIMARY KEY (job_id, artifact_type, uri)
            )"#
        ))
        .execute(&self.pool)
        .await
        .context("failed to create swiftpipe_artifacts")?;

        Ok(())
    }
}

impl SystemOfRecordSink for SqlxPostgresSink {
    fn upsert_job(&mut self, job: &JobRecord) -> Result<()> {
        let handle = tokio::runtime::Handle::try_current()
            .context("SqlxPostgresSink requires a tokio runtime")?;
        let schema = &self.schema;
        let pool = &self.pool;
        handle.block_on(async {
            sqlx::query(&format!(
                r#"INSERT INTO "{schema}".swiftpipe_jobs
                   (job_id, status, input_objects, messages, parse_errors,
                    rendered, output_prefix, processing_elapsed_ms, updated_at)
                   VALUES ($1, $2, $3, $4, $5, $6, $7, $8, now())
                   ON CONFLICT (job_id) DO UPDATE SET
                     status = EXCLUDED.status,
                     input_objects = EXCLUDED.input_objects,
                     messages = EXCLUDED.messages,
                     parse_errors = EXCLUDED.parse_errors,
                     rendered = EXCLUDED.rendered,
                     output_prefix = EXCLUDED.output_prefix,
                     processing_elapsed_ms = EXCLUDED.processing_elapsed_ms,
                     updated_at = now()"#
            ))
            .bind(&job.job_id)
            .bind(&job.status)
            .bind(job.input_objects as i64)
            .bind(job.messages as i64)
            .bind(job.parse_errors as i64)
            .bind(job.rendered as i64)
            .bind(&job.output_prefix)
            .bind(job.processing_elapsed_ms as i64)
            .execute(pool)
            .await
            .context("upsert_job failed")?;
            Ok(())
        })
    }

    fn append_audit_event(&mut self, event: &AuditEvent) -> Result<()> {
        let handle = tokio::runtime::Handle::try_current()
            .context("SqlxPostgresSink requires a tokio runtime")?;
        let schema = &self.schema;
        let pool = &self.pool;
        handle.block_on(async {
            sqlx::query(&format!(
                r#"INSERT INTO "{schema}".swiftpipe_audit_events (job_id, event_type, message)
                   VALUES ($1, $2, $3)"#
            ))
            .bind(&event.job_id)
            .bind(&event.event_type)
            .bind(&event.message)
            .execute(pool)
            .await
            .context("append_audit_event failed")?;
            Ok(())
        })
    }

    fn mark_artifact_committed(&mut self, artifact: &ArtifactRecord) -> Result<()> {
        let handle = tokio::runtime::Handle::try_current()
            .context("SqlxPostgresSink requires a tokio runtime")?;
        let schema = &self.schema;
        let pool = &self.pool;
        handle.block_on(async {
            sqlx::query(&format!(
                r#"INSERT INTO "{schema}".swiftpipe_artifacts (job_id, artifact_type, uri, committed_at)
                   VALUES ($1, $2, $3, now())
                   ON CONFLICT (job_id, artifact_type, uri) DO UPDATE SET committed_at = now()"#
            ))
            .bind(&artifact.job_id)
            .bind(&artifact.artifact_type)
            .bind(&artifact.uri)
            .execute(pool)
            .await
            .context("mark_artifact_committed failed")?;
            Ok(())
        })
    }
}

// ---------------------------------------------------------------------------
// SQL Server stub — not yet implemented
// ---------------------------------------------------------------------------

struct SqlServerSystemOfRecordSink;

impl SqlServerSystemOfRecordSink {
    fn connect(connection: &str, schema: &str) -> Result<Self> {
        let _ = (connection, schema);
        bail!(
            "sqlserver system-of-record sink is configured but not available yet; use postgres or file"
        )
    }
}

impl SystemOfRecordSink for SqlServerSystemOfRecordSink {
    fn upsert_job(&mut self, _job: &JobRecord) -> Result<()> {
        bail!("sqlserver system-of-record sink is not available")
    }

    fn append_audit_event(&mut self, _event: &AuditEvent) -> Result<()> {
        bail!("sqlserver system-of-record sink is not available")
    }

    fn mark_artifact_committed(&mut self, _artifact: &ArtifactRecord) -> Result<()> {
        bail!("sqlserver system-of-record sink is not available")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_sink_appends_job_event_and_artifact_records() {
        let path = std::env::temp_dir().join(format!(
            "swiftpipe-system-record-{}.jsonl",
            uuid::Uuid::new_v4()
        ));
        let mut sink = FileSystemOfRecordSink { path: path.clone() };

        sink.upsert_job(&JobRecord {
            job_id: "job-1".to_string(),
            status: "running".to_string(),
            input_objects: 2,
            messages: 0,
            parse_errors: 0,
            rendered: 0,
            output_prefix: "s3://out/jobs/job-1/".to_string(),
            processing_elapsed_ms: 0,
        })
        .expect("job");
        sink.append_audit_event(&AuditEvent {
            job_id: "job-1".to_string(),
            event_type: "started".to_string(),
            message: "processing started".to_string(),
        })
        .expect("event");
        sink.mark_artifact_committed(&ArtifactRecord {
            job_id: "job-1".to_string(),
            artifact_type: "manifest".to_string(),
            uri: "s3://out/jobs/job-1/manifest.json".to_string(),
        })
        .expect("artifact");

        let content = fs::read_to_string(&path).expect("jsonl");
        assert_eq!(content.lines().count(), 3);
        assert!(content.contains("\"kind\":\"job\""));
        assert!(content.contains("\"kind\":\"audit_event\""));
        assert!(content.contains("\"kind\":\"artifact\""));
        fs::remove_file(path).expect("cleanup");
    }
}
