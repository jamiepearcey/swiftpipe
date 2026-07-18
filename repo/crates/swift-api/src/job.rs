use anyhow::{bail, Context, Result};
use duckdb::Connection;
use std::error::Error;
use std::fmt;
use std::fs;
use std::io::{self, BufWriter, Seek, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Instant;
use swift_core::{parse_message, BlockId, ParsedMessage};
use swift_db::{materialize_message, InboundMessage, MigrationSink, ParsedOutputBatch, ParsedSink};
use swift_duckdb::{DuckDbExportFormat, DuckDbInboundConfig, DuckDbStore, ExportedTable};
use swift_schema::{
    infer_database_layout, render_message, DatabaseLayout, RenderEnvelope, RenderRequest,
    RenderRow, SchemaCatalog,
};
use uuid::Uuid;
use zip::write::SimpleFileOptions;

use crate::error::ApiErrorCode;
use crate::manifest::{
    job_output_uris, JobCounts, JobManifest, JobRequest, JobStatusArtifact, JobTimings,
    MessageResult, CONTRACT_VERSION,
};
use crate::object_store::{LocalObjectStore, ObjectStore};
use crate::state::{AppState, Metrics};
use crate::system_record::{
    open_system_of_record, ArtifactRecord, AuditEvent, JobRecord, SystemOfRecordConfig,
    SystemOfRecordSink,
};

type JobResult<T> = Result<T, JobError>;

#[derive(Debug)]
pub(crate) enum JobError {
    BadRequest(anyhow::Error),
    ZipLimitExceeded(anyhow::Error),
    Internal(anyhow::Error),
}

impl JobError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self::BadRequest(anyhow::anyhow!(message.into()))
    }

    fn zip_limit_exceeded(message: impl Into<String>) -> Self {
        Self::ZipLimitExceeded(anyhow::anyhow!(message.into()))
    }

    pub(crate) fn api_code(&self) -> ApiErrorCode {
        match self {
            Self::BadRequest(_) => ApiErrorCode::BadRequest,
            Self::ZipLimitExceeded(_) | Self::Internal(_) => ApiErrorCode::Internal,
        }
    }
}

impl fmt::Display for JobError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadRequest(error) | Self::ZipLimitExceeded(error) | Self::Internal(error) => {
                write!(f, "{error}")
            }
        }
    }
}

impl Error for JobError {}

impl From<anyhow::Error> for JobError {
    fn from(error: anyhow::Error) -> Self {
        Self::Internal(error)
    }
}

impl From<io::Error> for JobError {
    fn from(error: io::Error) -> Self {
        Self::Internal(error.into())
    }
}

impl From<std::path::StripPrefixError> for JobError {
    fn from(error: std::path::StripPrefixError) -> Self {
        Self::Internal(error.into())
    }
}

impl From<swift_schema::SchemaValidationError> for JobError {
    fn from(error: swift_schema::SchemaValidationError) -> Self {
        Self::Internal(error.into())
    }
}

impl From<duckdb::Error> for JobError {
    fn from(error: duckdb::Error) -> Self {
        Self::Internal(error.into())
    }
}

impl From<swift_duckdb::DuckDbAdapterError> for JobError {
    fn from(error: swift_duckdb::DuckDbAdapterError) -> Self {
        Self::Internal(error.into())
    }
}

impl From<serde_json::Error> for JobError {
    fn from(error: serde_json::Error) -> Self {
        Self::Internal(error.into())
    }
}

fn validate_object_uri(uri: &str) -> JobResult<()> {
    let Some(rest) = uri.strip_prefix("s3://") else {
        return Err(JobError::bad_request(format!(
            "unsupported object URI scheme for {uri}; expected s3://"
        )));
    };
    let mut parts = rest.split('/');
    let bucket = parts.next().unwrap_or_default();
    if bucket.is_empty() || bucket == "." || bucket == ".." {
        return Err(JobError::bad_request(format!(
            "invalid object URI bucket in {uri}"
        )));
    }
    for part in parts {
        if part.is_empty() {
            continue;
        }
        if part == "." || part == ".." || part.contains('\\') {
            return Err(JobError::bad_request(format!(
                "invalid object URI path segment in {uri}"
            )));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct OutputSelection {
    rendered: bool,
    parquet: bool,
    errors: bool,
    zip: bool,
}

impl OutputSelection {
    pub(crate) fn all() -> Self {
        Self {
            rendered: true,
            parquet: true,
            errors: true,
            zip: true,
        }
    }

    fn from_request(outputs: Option<Vec<String>>) -> JobResult<Self> {
        let Some(outputs) = outputs else {
            return Ok(Self::all());
        };
        let mut selection = Self {
            rendered: false,
            parquet: false,
            errors: false,
            zip: false,
        };
        for output in outputs {
            match output.as_str() {
                "rendered" | "fin" => selection.rendered = true,
                "parquet" | "normalized_parquet" => selection.parquet = true,
                "errors" | "errors_ndjson" => selection.errors = true,
                "zip" => selection.zip = true,
                "all" => selection = Self::all(),
                "manifest" => {}
                other => {
                    return Err(JobError::bad_request(format!(
                        "unknown output requested: {other}"
                    )));
                }
            }
        }
        Ok(selection)
    }

    fn needs_duckdb(self) -> bool {
        self.parquet
    }
}

pub(crate) fn process_upload(
    state: &AppState,
    job_id: String,
    body: Vec<u8>,
    message_type: Option<String>,
    outputs: Option<Vec<String>>,
) -> JobResult<JobManifest> {
    let store = LocalObjectStore::new(&state.object_root);
    let input_uri = format!("s3://swiftpipe-inbox/uploads/{job_id}.fin");
    put_object(&store, &state.metrics, &input_uri, &body)?;
    let output_prefix = format!("s3://swiftpipe-outbox/jobs/{job_id}/");
    process_job(
        state,
        &store,
        JobProcessRequest {
            job_id,
            input_uris: vec![input_uri],
            output_prefix,
            configured_message_type: message_type,
            render_validate: true,
            output_selection: OutputSelection::from_request(outputs)?,
        },
    )
}

fn get_object(store: &LocalObjectStore, metrics: &Metrics, uri: &str) -> Result<Vec<u8>> {
    let bytes = store.get(uri)?;
    metrics.object_store_op("get");
    Ok(bytes)
}

fn put_object(store: &LocalObjectStore, metrics: &Metrics, uri: &str, bytes: &[u8]) -> Result<()> {
    store.put_atomic(uri, bytes)?;
    metrics.object_store_op("put");
    Ok(())
}

fn list_objects(
    store: &LocalObjectStore,
    metrics: &Metrics,
    prefix_uri: &str,
    include_suffix: &str,
) -> Result<Vec<String>> {
    let uris = store.list(prefix_uri, include_suffix)?;
    metrics.object_store_op("list");
    Ok(uris)
}

pub(crate) fn process_job_request(
    state: &AppState,
    job_id: String,
    request: JobRequest,
) -> JobResult<JobManifest> {
    let store = LocalObjectStore::new(&state.object_root);
    let output_prefix = request
        .output_prefix
        .unwrap_or_else(|| format!("s3://swiftpipe-outbox/jobs/{job_id}/"));
    validate_object_uri(&output_prefix)?;
    let mut prefix_fanout = None;
    let input_uris = if let Some(input_uri) = request.input_uri {
        validate_object_uri(&input_uri)?;
        vec![input_uri]
    } else if let Some(prefix) = request.input_prefix {
        validate_object_uri(&prefix)?;
        let matches = list_objects(
            &store,
            &state.metrics,
            &prefix,
            request.include_suffix.as_deref().unwrap_or(".fin"),
        )?;
        if matches.len() > state.max_prefix_fanout {
            return Err(JobError::bad_request(format!(
                "input prefix matched {} objects, exceeding max_prefix_fanout {}; use --include-suffix filtering or smaller prefixes",
                matches.len(),
                state.max_prefix_fanout
            )));
        }
        prefix_fanout = Some(matches.len());
        matches
    } else {
        return Err(JobError::bad_request(
            "job request requires input_uri or input_prefix",
        ));
    };
    if input_uris.is_empty() {
        return Err(JobError::bad_request("input selection matched no objects"));
    }
    if let Some(fanout) = prefix_fanout {
        state.metrics.prefix_job_fanout_objects(fanout);
    }
    process_job(
        state,
        &store,
        JobProcessRequest {
            job_id,
            input_uris,
            output_prefix,
            configured_message_type: request.message_type,
            render_validate: request.render_validate.unwrap_or(true),
            output_selection: OutputSelection::from_request(request.outputs)?,
        },
    )
}

pub(crate) struct JobProcessRequest {
    pub(crate) job_id: String,
    pub(crate) input_uris: Vec<String>,
    pub(crate) output_prefix: String,
    pub(crate) configured_message_type: Option<String>,
    pub(crate) render_validate: bool,
    pub(crate) output_selection: OutputSelection,
}

pub(crate) fn process_job(
    state: &AppState,
    store: &LocalObjectStore,
    request: JobProcessRequest,
) -> JobResult<JobManifest> {
    let JobProcessRequest {
        job_id,
        input_uris,
        output_prefix,
        configured_message_type,
        render_validate,
        output_selection,
    } = request;
    let job_started = Instant::now();
    match process_job_inner(
        state,
        store,
        JobProcessInner {
            job_id: job_id.clone(),
            input_uris: input_uris.clone(),
            output_prefix: output_prefix.clone(),
            configured_message_type,
            render_validate,
            output_selection,
            job_started,
        },
    ) {
        Ok(manifest) => Ok(manifest),
        Err(error) => {
            let error_message = error.to_string();
            let outputs = job_output_uris(&output_prefix);
            let manifest = JobManifest {
                contract_version: CONTRACT_VERSION,
                job_id: job_id.clone(),
                status: "failed".to_string(),
                processing_elapsed_ms: elapsed_ms(job_started),
                timings: JobTimings::default(),
                input_uris: input_uris.clone(),
                output_prefix: output_prefix.clone(),
                outputs,
                counts: JobCounts {
                    input_objects: input_uris.len(),
                    ..JobCounts::default()
                },
                messages: Vec::new(),
            };
            put_object(
                store,
                &state.metrics,
                &manifest.outputs.manifest,
                serde_json::to_vec_pretty(&manifest)?.as_slice(),
            )?;
            write_status(
                store,
                &state.metrics,
                &output_prefix,
                &job_id,
                "failed",
                &manifest.outputs.manifest,
                Some(error_message.clone()),
            )?;
            let mut system_record = open_traced_system_of_record(&state.system_of_record)?;
            system_record.upsert_job(&job_record_from_manifest(&manifest))?;
            system_record.append_audit_event(&AuditEvent {
                job_id,
                event_type: "failed".to_string(),
                message: error_message.clone(),
            })?;
            system_record.mark_artifact_committed(&ArtifactRecord {
                job_id: manifest.job_id.clone(),
                artifact_type: "manifest".to_string(),
                uri: manifest.outputs.manifest.clone(),
            })?;
            system_record.mark_artifact_committed(&ArtifactRecord {
                job_id: manifest.job_id.clone(),
                artifact_type: "status".to_string(),
                uri: format!("{output_prefix}status.json"),
            })?;
            Err(error)
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ZipLimits {
    max_total_bytes: u64,
    max_entries: usize,
}

impl ZipLimits {
    fn from_state(state: &AppState) -> Self {
        Self {
            max_total_bytes: state.zip_max_total_bytes,
            max_entries: state.zip_max_entries,
        }
    }
}

struct JobProcessInner {
    job_id: String,
    input_uris: Vec<String>,
    output_prefix: String,
    configured_message_type: Option<String>,
    render_validate: bool,
    output_selection: OutputSelection,
    job_started: Instant,
}

struct TracedSystemOfRecordSink {
    inner: Box<dyn SystemOfRecordSink>,
}

impl fmt::Debug for TracedSystemOfRecordSink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TracedSystemOfRecordSink")
            .finish_non_exhaustive()
    }
}

fn open_traced_system_of_record(config: &SystemOfRecordConfig) -> Result<TracedSystemOfRecordSink> {
    Ok(TracedSystemOfRecordSink {
        inner: open_system_of_record(config)?,
    })
}

impl TracedSystemOfRecordSink {
    fn upsert_job(&mut self, job: &JobRecord) -> Result<()> {
        let span = tracing::info_span!(
            "swiftpipe.system_record_write",
            operation = "upsert_job",
            job_id = %job.job_id,
            status = %job.status,
            input_objects = job.input_objects,
            messages = job.messages,
            parse_errors = job.parse_errors,
            rendered = job.rendered,
        );
        span.in_scope(|| self.inner.upsert_job(job))
    }

    fn append_audit_event(&mut self, event: &AuditEvent) -> Result<()> {
        let span = tracing::info_span!(
            "swiftpipe.system_record_write",
            operation = "append_audit_event",
            job_id = %event.job_id,
            event_type = %event.event_type,
        );
        span.in_scope(|| self.inner.append_audit_event(event))
    }

    fn mark_artifact_committed(&mut self, artifact: &ArtifactRecord) -> Result<()> {
        let span = tracing::info_span!(
            "swiftpipe.system_record_write",
            operation = "mark_artifact_committed",
            job_id = %artifact.job_id,
            artifact_type = %artifact.artifact_type,
            uri = %artifact.uri,
        );
        span.in_scope(|| self.inner.mark_artifact_committed(artifact))
    }
}

fn process_job_inner(
    state: &AppState,
    store: &LocalObjectStore,
    request: JobProcessInner,
) -> JobResult<JobManifest> {
    let JobProcessInner {
        job_id,
        input_uris,
        output_prefix,
        configured_message_type,
        render_validate,
        output_selection,
        job_started,
    } = request;
    let setup_started = Instant::now();
    let outputs = job_output_uris(&output_prefix);
    write_status(
        store,
        &state.metrics,
        &output_prefix,
        &job_id,
        "running",
        &outputs.manifest,
        None,
    )?;
    put_object(
        store,
        &state.metrics,
        &format!("{output_prefix}input_snapshot.json"),
        &serde_json::to_vec_pretty(&input_uris)?,
    )?;
    let mut system_record = open_traced_system_of_record(&state.system_of_record)?;
    system_record.upsert_job(&JobRecord {
        job_id: job_id.clone(),
        status: "running".to_string(),
        input_objects: input_uris.len(),
        messages: 0,
        parse_errors: 0,
        rendered: 0,
        output_prefix: output_prefix.clone(),
        processing_elapsed_ms: 0,
    })?;
    system_record.append_audit_event(&AuditEvent {
        job_id: job_id.clone(),
        event_type: "started".to_string(),
        message: format!("processing {} input object(s)", input_uris.len()),
    })?;
    system_record.mark_artifact_committed(&ArtifactRecord {
        job_id: job_id.clone(),
        artifact_type: "input_snapshot".to_string(),
        uri: format!("{output_prefix}input_snapshot.json"),
    })?;

    let catalog = &state.schema_catalog;
    let layout = infer_database_layout(catalog.as_ref());
    let mut duckdb = if output_selection.needs_duckdb() {
        let work_dir = state.work_root.join(&job_id);
        fs::create_dir_all(&work_dir)?;
        let db_path = work_dir.join("hydrate.duckdb");
        let conn = Connection::open(&db_path)?;
        let mut duckdb = DuckDbStore::new(conn, DuckDbInboundConfig::default());
        duckdb.apply_layout(&layout)?;
        Some(duckdb)
    } else {
        None
    };
    let setup_ms = elapsed_ms(setup_started);

    let prepare_started = Instant::now();
    let prepared_inputs = prepare_inputs(PrepareInputs {
        store,
        catalog: catalog.as_ref(),
        input_uris: &input_uris,
        output_prefix: &output_prefix,
        configured_message_type: configured_message_type.as_deref(),
        render_validate,
        render_output: output_selection.rendered,
        persist_raw_text: state.persist_raw_text,
        persist_raw_fields: state.persist_raw_fields,
        max_parallelism: state.max_prefix_parallelism,
        metrics: &state.metrics,
    })?;
    let prepare_ms = elapsed_ms(prepare_started);

    let mut results = Vec::new();
    let mut counts = JobCounts {
        input_objects: input_uris.len(),
        ..JobCounts::default()
    };

    let mut hydration_batch = ParsedOutputBatch::empty();
    for prepared in prepared_inputs {
        let input_result = match prepared {
            PreparedInput::Completed {
                result,
                batch,
                rendered_uri,
                rendered,
            } => {
                let write_rendered = match (output_selection.rendered, &rendered_uri, &rendered) {
                    (true, Some(rendered_uri), Some(rendered)) => {
                        write_rendered_output(store, &state.metrics, rendered_uri, rendered)
                            .and_then(|()| {
                                system_record.mark_artifact_committed(&ArtifactRecord {
                                    job_id: job_id.clone(),
                                    artifact_type: "rendered".to_string(),
                                    uri: rendered_uri.clone(),
                                })
                            })
                    }
                    _ => Ok(()),
                };
                match write_rendered {
                    Ok(()) => {
                        hydration_batch.extend(batch);
                        result
                    }
                    Err(error) => failed_message_result(
                        result.message_id,
                        result.message_type,
                        result.input_uri,
                        result.elapsed_ms,
                        error,
                    ),
                }
            }
            PreparedInput::Failed { result } => result,
        };
        if input_result.status == "completed" {
            counts.messages += 1;
            counts.parse_errors += input_result.parse_errors;
            counts.rendered += usize::from(input_result.rendered_uri.is_some());
            state.metrics.message_processed(&input_result.message_type);
        }
        results.push(input_result);
    }
    let hydrate_write_started = Instant::now();
    let hydrate_write_ms = if let Some(duckdb) = &mut duckdb {
        write_duckdb_batch(duckdb, &hydration_batch, &job_id, &state.metrics)?;
        elapsed_ms(hydrate_write_started)
    } else {
        0
    };

    let export_started = Instant::now();
    let mut exported_count = 0;
    if output_selection.parquet {
        let parquet_prefix = outputs.normalized_parquet_prefix.clone();
        let parquet_dir = store.local_path_for_export(&parquet_prefix)?;
        let duckdb = duckdb
            .as_ref()
            .context("parquet output requested without duckdb hydration")?;
        let (tables_exported, bytes_written) =
            export_parquet_tables(duckdb, &layout, &parquet_dir, &job_id)?;
        state.metrics.parquet_bytes_written(bytes_written);
        exported_count = tables_exported;
        system_record.mark_artifact_committed(&ArtifactRecord {
            job_id: job_id.clone(),
            artifact_type: "parquet".to_string(),
            uri: outputs.normalized_parquet_prefix.clone(),
        })?;
    }
    if output_selection.errors {
        write_errors_ndjson_from_batch(
            &hydration_batch,
            &results,
            store,
            &state.metrics,
            &outputs.errors_ndjson,
        )?;
        system_record.mark_artifact_committed(&ArtifactRecord {
            job_id: job_id.clone(),
            artifact_type: "errors".to_string(),
            uri: outputs.errors_ndjson.clone(),
        })?;
    }
    let export_ms = elapsed_ms(export_started);

    let status = if results.iter().any(|result| result.status == "failed") {
        "completed_with_errors"
    } else {
        "completed"
    };
    let manifest = JobManifest {
        contract_version: CONTRACT_VERSION,
        job_id,
        status: status.to_string(),
        processing_elapsed_ms: elapsed_ms(job_started),
        timings: JobTimings {
            setup_ms,
            prepare_ms,
            hydrate_write_ms,
            export_ms,
            manifest_ms: 0,
            zip_ms: 0,
        },
        input_uris,
        output_prefix: output_prefix.clone(),
        outputs,
        counts,
        messages: results,
    };
    let manifest_started = Instant::now();
    put_object(
        store,
        &state.metrics,
        &manifest.outputs.manifest,
        serde_json::to_vec_pretty(&manifest)?.as_slice(),
    )?;
    system_record.mark_artifact_committed(&ArtifactRecord {
        job_id: manifest.job_id.clone(),
        artifact_type: "manifest".to_string(),
        uri: manifest.outputs.manifest.clone(),
    })?;
    write_status(
        store,
        &state.metrics,
        &output_prefix,
        &manifest.job_id,
        &manifest.status,
        &manifest.outputs.manifest,
        None,
    )?;
    system_record.mark_artifact_committed(&ArtifactRecord {
        job_id: manifest.job_id.clone(),
        artifact_type: "status".to_string(),
        uri: format!("{output_prefix}status.json"),
    })?;
    let manifest_ms = elapsed_ms(manifest_started);
    let zip_started = Instant::now();
    let zip_ms = if output_selection.zip {
        write_zip_for_prefix(
            store,
            &output_prefix,
            &manifest.outputs.zip,
            ZipLimits::from_state(state),
        )?;
        state.metrics.object_store_op("put");
        system_record.mark_artifact_committed(&ArtifactRecord {
            job_id: manifest.job_id.clone(),
            artifact_type: "zip".to_string(),
            uri: manifest.outputs.zip.clone(),
        })?;
        elapsed_ms(zip_started)
    } else {
        0
    };
    let mut manifest = manifest;
    manifest.processing_elapsed_ms = elapsed_ms(job_started);
    manifest.timings.manifest_ms = manifest_ms;
    manifest.timings.zip_ms = zip_ms;
    put_object(
        store,
        &state.metrics,
        &manifest.outputs.manifest,
        serde_json::to_vec_pretty(&manifest)?.as_slice(),
    )?;
    system_record.upsert_job(&job_record_from_manifest(&manifest))?;
    system_record.append_audit_event(&AuditEvent {
        job_id: manifest.job_id.clone(),
        event_type: manifest.status.clone(),
        message: format!(
            "processed {} message(s), {} parse error(s), {} rendered",
            manifest.counts.messages, manifest.counts.parse_errors, manifest.counts.rendered
        ),
    })?;
    write_status(
        store,
        &state.metrics,
        &output_prefix,
        &manifest.job_id,
        &manifest.status,
        &manifest.outputs.manifest,
        None,
    )?;
    JobEvent::completed(&manifest, exported_count).emit();
    Ok(manifest)
}

struct JobEvent<'a> {
    job_id: &'a str,
    status: &'a str,
    input_objects: usize,
    messages: usize,
    parse_errors: usize,
    rendered: usize,
    exported_tables: usize,
    processing_elapsed_ms: u64,
}

impl<'a> JobEvent<'a> {
    fn completed(manifest: &'a JobManifest, exported_tables: usize) -> Self {
        Self {
            job_id: &manifest.job_id,
            status: &manifest.status,
            input_objects: manifest.counts.input_objects,
            messages: manifest.counts.messages,
            parse_errors: manifest.counts.parse_errors,
            rendered: manifest.counts.rendered,
            exported_tables,
            processing_elapsed_ms: manifest.processing_elapsed_ms,
        }
    }

    fn emit(&self) {
        tracing::info!(
            target: "swift_api::job",
            event = "swiftpipe.job_event",
            job_id = %self.job_id,
            status = %self.status,
            input_objects = self.input_objects,
            messages = self.messages,
            parse_errors = self.parse_errors,
            rendered = self.rendered,
            exported_tables = self.exported_tables,
            processing_elapsed_ms = self.processing_elapsed_ms,
        );
    }
}

enum PreparedInput {
    Completed {
        result: MessageResult,
        batch: ParsedOutputBatch,
        rendered_uri: Option<String>,
        rendered: Option<String>,
    },
    Failed {
        result: MessageResult,
    },
}

struct ProcessInput<'a> {
    store: &'a LocalObjectStore,
    metrics: &'a Metrics,
    catalog: &'a SchemaCatalog,
    input_uri: &'a str,
    output_prefix: &'a str,
    message_id: &'a str,
    configured_message_type: Option<&'a str>,
    render_validate: bool,
    render_output: bool,
    persist_raw_text: bool,
    persist_raw_fields: bool,
}

fn process_input(request: ProcessInput<'_>) -> Result<PreparedInput> {
    let ProcessInput {
        store,
        metrics,
        catalog,
        input_uri,
        output_prefix,
        message_id,
        configured_message_type,
        render_validate,
        render_output,
        persist_raw_text,
        persist_raw_fields,
    } = request;
    let bytes = get_object(store, metrics, input_uri)?;
    let parsed = parse_input_message(&bytes, input_uri, message_id);
    let message_type = configured_message_type
        .map(str::to_string)
        .or_else(|| infer_message_type(&parsed))
        .context("message_type was not supplied and could not be inferred from block 2")?;
    let envelope = source_envelope(&parsed, &message_type);
    let raw_body = if persist_raw_text {
        String::from_utf8(bytes.clone()).context("FIN input must be utf-8 text")?
    } else {
        String::new()
    };
    let inbound = InboundMessage {
        id: message_id.to_string(),
        message_type: message_type.clone(),
        body: raw_body,
    };
    let mut batch = materialize_input_message(catalog, &inbound, &parsed, input_uri)
        .with_context(|| format!("failed to materialize {message_id} ({message_type})"))?;
    if !persist_raw_fields {
        for field in &mut batch.fields {
            field.raw_value.clear();
        }
    }
    let parse_errors = batch.parse_errors.len();
    let render_rows = batch_render_rows(&batch);

    let (rendered_uri, rendered) = if render_output {
        let rendered = render_roundtrip(
            catalog,
            message_id,
            &message_type,
            envelope,
            render_rows,
            render_validate,
        )
        .with_context(|| format!("failed to render {message_id} ({message_type})"))?;
        (
            Some(format!("{output_prefix}rendered/{message_id}.fin")),
            Some(rendered),
        )
    } else {
        (None, None)
    };

    Ok(PreparedInput::Completed {
        result: MessageResult {
            message_id: message_id.to_string(),
            message_type,
            input_uri: input_uri.to_string(),
            status: "completed".to_string(),
            elapsed_ms: 0,
            parse_errors,
            rendered_uri: rendered_uri.clone(),
            error: None,
        },
        batch,
        rendered_uri,
        rendered,
    })
}

struct PrepareInputs<'a> {
    store: &'a LocalObjectStore,
    metrics: &'a Metrics,
    catalog: &'a SchemaCatalog,
    input_uris: &'a [String],
    output_prefix: &'a str,
    configured_message_type: Option<&'a str>,
    render_validate: bool,
    render_output: bool,
    persist_raw_text: bool,
    persist_raw_fields: bool,
    max_parallelism: usize,
}

fn prepare_inputs(request: PrepareInputs<'_>) -> Result<Vec<PreparedInput>> {
    let PrepareInputs {
        store,
        metrics,
        catalog,
        input_uris,
        output_prefix,
        configured_message_type,
        render_validate,
        render_output,
        persist_raw_text,
        persist_raw_fields,
        max_parallelism,
    } = request;
    if input_uris.is_empty() {
        return Ok(Vec::new());
    }
    let workers = thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .min(max_parallelism.max(1))
        .min(input_uris.len());
    let mut prepared = thread::scope(|scope| {
        let mut handles = Vec::new();
        for worker in 0..workers {
            handles.push(scope.spawn(move || {
                let mut chunk = Vec::new();
                for index in (worker..input_uris.len()).step_by(workers) {
                    let input_started = Instant::now();
                    let input_uri = &input_uris[index];
                    let message_id = format!("msg-{}", index + 1);
                    let prepared = match process_input(ProcessInput {
                        store,
                        metrics,
                        catalog,
                        input_uri,
                        output_prefix,
                        message_id: &message_id,
                        configured_message_type,
                        render_validate,
                        render_output,
                        persist_raw_text,
                        persist_raw_fields,
                    }) {
                        Ok(mut prepared) => {
                            if let PreparedInput::Completed { result, .. } = &mut prepared {
                                result.elapsed_ms = elapsed_ms(input_started);
                            }
                            prepared
                        }
                        Err(error) => PreparedInput::Failed {
                            result: failed_message_result(
                                message_id,
                                configured_message_type
                                    .map_or_else(|| "unknown".to_string(), str::to_string),
                                input_uri.clone(),
                                elapsed_ms(input_started),
                                error,
                            ),
                        },
                    };
                    chunk.push((index, prepared));
                }
                chunk
            }));
        }
        let mut prepared = Vec::new();
        for handle in handles {
            let chunk = handle.join().map_err(|payload| {
                anyhow::anyhow!(
                    "input worker thread panicked: {}",
                    panic_payload_message(&*payload)
                )
            })?;
            prepared.extend(chunk);
        }
        Ok::<_, anyhow::Error>(prepared)
    })?;
    prepared.sort_by_key(|(index, _)| *index);
    Ok(prepared.into_iter().map(|(_, prepared)| prepared).collect())
}

fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic payload".to_string()
    }
}

fn write_rendered_output(
    store: &LocalObjectStore,
    metrics: &Metrics,
    rendered_uri: &str,
    rendered: &str,
) -> Result<()> {
    put_object(store, metrics, rendered_uri, rendered.as_bytes())?;
    Ok(())
}

fn failed_message_result(
    message_id: String,
    message_type: String,
    input_uri: String,
    elapsed_ms: u64,
    error: anyhow::Error,
) -> MessageResult {
    MessageResult {
        message_id,
        message_type,
        input_uri,
        status: "failed".to_string(),
        elapsed_ms,
        parse_errors: 0,
        rendered_uri: None,
        error: Some(error.to_string()),
    }
}

fn job_record_from_manifest(manifest: &JobManifest) -> JobRecord {
    JobRecord {
        job_id: manifest.job_id.clone(),
        status: manifest.status.clone(),
        input_objects: manifest.counts.input_objects,
        messages: manifest.counts.messages,
        parse_errors: manifest.counts.parse_errors,
        rendered: manifest.counts.rendered,
        output_prefix: manifest.output_prefix.clone(),
        processing_elapsed_ms: manifest.processing_elapsed_ms,
    }
}

fn write_status(
    store: &LocalObjectStore,
    metrics: &Metrics,
    output_prefix: &str,
    job_id: &str,
    status: &str,
    manifest_uri: &str,
    error: Option<String>,
) -> Result<()> {
    let status = JobStatusArtifact {
        job_id: job_id.to_string(),
        status: status.to_string(),
        manifest_uri: manifest_uri.to_string(),
        error,
    };
    put_object(
        store,
        metrics,
        &format!("{output_prefix}status.json"),
        serde_json::to_vec_pretty(&status)?.as_slice(),
    )
}

fn parse_input_message<'a>(
    bytes: &'a [u8],
    input_uri: &str,
    message_id: &str,
) -> ParsedMessage<'a> {
    let span = tracing::info_span!(
        "swiftpipe.parse_message",
        input_uri = %input_uri,
        message_id = %message_id,
        input_bytes = bytes.len(),
    );
    span.in_scope(|| parse_message(bytes))
}

fn materialize_input_message(
    catalog: &SchemaCatalog,
    inbound: &InboundMessage,
    parsed: &ParsedMessage<'_>,
    input_uri: &str,
) -> Result<ParsedOutputBatch> {
    let span = tracing::info_span!(
        "swiftpipe.schema_materialize",
        input_uri = %input_uri,
        message_id = %inbound.id,
        message_type = %inbound.message_type,
        parsed_blocks = parsed.blocks.len(),
    );
    Ok(span.in_scope(|| materialize_message(catalog, inbound, parsed))?)
}

fn write_duckdb_batch<S>(
    duckdb: &mut S,
    batch: &ParsedOutputBatch,
    job_id: &str,
    metrics: &Metrics,
) -> Result<()>
where
    S: ParsedSink,
    S::Error: Error + Send + Sync + 'static,
{
    let rows_written = parsed_output_row_count(batch);
    let span = tracing::info_span!(
        "swiftpipe.duckdb_write",
        job_id = %job_id,
        rows_written,
        raw_messages = batch.raw_messages.len(),
        fields = batch.fields.len(),
        parse_errors = batch.parse_errors.len(),
        normalized_rows = batch.normalized_rows.len(),
    );
    span.in_scope(|| duckdb.write_batch(batch))?;
    metrics.duckdb_rows_written(u64::try_from(rows_written).unwrap_or(u64::MAX));
    Ok(())
}

fn parsed_output_row_count(batch: &ParsedOutputBatch) -> usize {
    batch.raw_messages.len()
        + batch.fields.len()
        + batch.parse_errors.len()
        + batch.normalized_rows.len()
}

trait ParquetExporter {
    type Error;

    fn export_parquet_layout(
        &self,
        layout: &DatabaseLayout,
        directory: &Path,
    ) -> std::result::Result<Vec<ExportedTable>, Self::Error>;
}

impl ParquetExporter for DuckDbStore {
    type Error = swift_duckdb::DuckDbAdapterError;

    fn export_parquet_layout(
        &self,
        layout: &DatabaseLayout,
        directory: &Path,
    ) -> std::result::Result<Vec<ExportedTable>, Self::Error> {
        self.export_layout(layout, directory, &[DuckDbExportFormat::Parquet])
    }
}

fn export_parquet_tables<E>(
    exporter: &E,
    layout: &DatabaseLayout,
    parquet_dir: &Path,
    job_id: &str,
) -> Result<(usize, u64)>
where
    E: ParquetExporter,
    E::Error: Error + Send + Sync + 'static,
{
    let span = tracing::info_span!(
        "swiftpipe.parquet_export",
        job_id = %job_id,
        output_dir = %parquet_dir.display(),
        tables_exported = tracing::field::Empty,
        bytes_written = tracing::field::Empty,
    );
    let exported = span.in_scope(|| exporter.export_parquet_layout(layout, parquet_dir))?;
    let bytes_written = exported
        .iter()
        .map(|table| Ok(fs::metadata(&table.path)?.len()))
        .sum::<Result<u64>>()?;
    span.record("tables_exported", exported.len());
    span.record("bytes_written", bytes_written);
    Ok((exported.len(), bytes_written))
}

fn source_envelope(parsed: &swift_core::ParsedMessage<'_>, message_type: &str) -> RenderEnvelope {
    RenderEnvelope {
        block1: block_content(parsed, BlockId::BasicHeader)
            .unwrap_or_else(|| "F01BANKBEBBAXXX0000000000".to_string()),
        block2: block_content(parsed, BlockId::ApplicationHeader)
            .unwrap_or_else(|| format!("I{}BANKDEFFXXXXN", message_type.trim_start_matches("MT"))),
        block3: block_content(parsed, BlockId::UserHeader),
        block5: block_content(parsed, BlockId::Trailer),
    }
}

fn block_content(parsed: &swift_core::ParsedMessage<'_>, id: BlockId<'_>) -> Option<String> {
    parsed
        .blocks
        .iter()
        .find(|block| block.id == id)
        .and_then(|block| std::str::from_utf8(block.content).ok())
        .map(str::to_string)
}

fn render_roundtrip(
    catalog: &SchemaCatalog,
    message_id: &str,
    message_type: &str,
    envelope: RenderEnvelope,
    rows: Vec<RenderRow>,
    validate: bool,
) -> Result<String> {
    let rendered = render_message(
        catalog,
        &RenderRequest {
            message_id: message_id.to_string(),
            message_type: message_type.to_string(),
            envelope,
            rows,
        },
    )?;
    if validate {
        let parsed = parse_message(rendered.as_bytes());
        if !parsed.diagnostics.is_empty() {
            bail!(
                "rendered message has parse diagnostics: {:?}",
                parsed.diagnostics
            );
        }
    }
    Ok(rendered)
}

fn batch_render_rows(batch: &swift_db::ParsedOutputBatch) -> Vec<RenderRow> {
    batch
        .normalized_rows
        .iter()
        .map(|row| RenderRow {
            table: row.table.clone(),
            values: row.values.clone(),
        })
        .collect()
}

fn write_errors_ndjson_from_batch(
    batch: &ParsedOutputBatch,
    results: &[MessageResult],
    store: &LocalObjectStore,
    metrics: &Metrics,
    uri: &str,
) -> Result<()> {
    let mut output = String::new();
    for row in &batch.parse_errors {
        output.push_str(&serde_json::to_string(&serde_json::json!({
            "message_id": row.message_id,
            "error": row.error,
        }))?);
        output.push('\n');
    }
    for result in results {
        let Some(error) = &result.error else {
            continue;
        };
        output.push_str(&serde_json::to_string(&serde_json::json!({
            "message_id": result.message_id,
            "error": error,
        }))?);
        output.push('\n');
    }
    put_object(store, metrics, uri, output.as_bytes())
}

#[derive(Debug)]
struct CountingWriter {
    inner: BufWriter<fs::File>,
    bytes_written: u64,
}

impl CountingWriter {
    fn new(inner: BufWriter<fs::File>) -> Self {
        Self {
            inner,
            bytes_written: 0,
        }
    }
}

impl Write for CountingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(buf)?;
        self.bytes_written = self
            .bytes_written
            .saturating_add(u64::try_from(written).unwrap_or(u64::MAX));
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl Seek for CountingWriter {
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        self.inner.seek(pos)
    }
}

fn write_zip_for_prefix(
    store: &LocalObjectStore,
    prefix_uri: &str,
    zip_uri: &str,
    limits: ZipLimits,
) -> JobResult<()> {
    let prefix_path = store.local_path_for_export(prefix_uri)?;
    let zip_path = store.local_path_for_export(zip_uri)?;
    let span = tracing::info_span!(
        "swiftpipe.zip_create",
        prefix_uri = %prefix_uri,
        zip_uri = %zip_uri,
        files_zipped = tracing::field::Empty,
        bytes_written = tracing::field::Empty,
    );
    let mut files_zipped = 0;
    let result = span.in_scope(|| {
        let temp_zip_path = temp_output_path(&zip_path)?;
        let write_result = write_zip_temp(
            &prefix_path,
            &prefix_path,
            &temp_zip_path,
            limits,
            &mut files_zipped,
        );
        if let Err(error) = write_result {
            let _ = fs::remove_file(&temp_zip_path);
            return Err(error);
        }
        fs::rename(&temp_zip_path, &zip_path).map_err(|error| {
            let _ = fs::remove_file(&temp_zip_path);
            JobError::Internal(anyhow::anyhow!(error).context(format!(
                "failed to move temporary output to {}",
                zip_path.display()
            )))
        })
    });
    if let Err(JobError::ZipLimitExceeded(error)) = &result {
        tracing::warn!(
            target: "swift_api::job",
            event = "swiftpipe.zip_limit_exceeded",
            prefix_uri = %prefix_uri,
            zip_uri = %zip_uri,
            max_zip_total_bytes = limits.max_total_bytes,
            max_zip_entries = limits.max_entries,
            error = %error,
            "zip export limit exceeded"
        );
    }
    result?;
    let bytes_written = fs::metadata(&zip_path)?.len();
    span.record("files_zipped", files_zipped);
    span.record("bytes_written", bytes_written);
    Ok(())
}

#[allow(dead_code)]
pub(crate) fn write_zip_for_bench(
    object_root: &Path,
    prefix_uri: &str,
    zip_uri: &str,
    max_total_bytes: u64,
    max_entries: usize,
) -> JobResult<()> {
    let store = LocalObjectStore::new(object_root);
    write_zip_for_prefix(
        &store,
        prefix_uri,
        zip_uri,
        ZipLimits {
            max_total_bytes,
            max_entries,
        },
    )
}

fn temp_output_path(path: &Path) -> JobResult<PathBuf> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|error| JobError::Internal(error.into()))?;
    let filename = path
        .file_name()
        .and_then(|filename| filename.to_str())
        .ok_or_else(|| {
            JobError::Internal(anyhow::anyhow!(
                "output path must include a valid file name"
            ))
        })?;
    Ok(parent.join(format!(
        ".{filename}.swiftpipe-tmp-{}-{}",
        std::process::id(),
        Uuid::new_v4()
    )))
}

fn write_zip_temp(
    prefix_path: &Path,
    root: &Path,
    temp_zip_path: &Path,
    limits: ZipLimits,
    files_zipped: &mut usize,
) -> JobResult<()> {
    let file = fs::File::create(temp_zip_path).map_err(|error| JobError::Internal(error.into()))?;
    let writer = CountingWriter::new(BufWriter::new(file));
    let mut zip = zip::ZipWriter::new(writer);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    *files_zipped = if prefix_path.exists() {
        add_dir_to_zip(&mut zip, root, prefix_path, temp_zip_path, options, limits)?
    } else {
        0
    };
    let writer = zip.finish().map_err(map_zip_error)?;
    if writer.bytes_written > limits.max_total_bytes {
        return Err(JobError::zip_limit_exceeded(format!(
            "zip output exceeded max_zip_total_bytes {}",
            limits.max_total_bytes
        )));
    }
    Ok(())
}

fn add_dir_to_zip(
    zip: &mut zip::ZipWriter<CountingWriter>,
    root: &Path,
    dir: &Path,
    zip_path: &Path,
    options: SimpleFileOptions,
    limits: ZipLimits,
) -> JobResult<usize> {
    let mut files_zipped = 0;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path == zip_path {
            continue;
        }
        if path.is_dir() {
            files_zipped += add_dir_to_zip(zip, root, &path, zip_path, options, limits)?;
            continue;
        }
        if files_zipped >= limits.max_entries {
            return Err(JobError::zip_limit_exceeded(format!(
                "zip entry count exceeded max_zip_entries {}",
                limits.max_entries
            )));
        }
        let name = path
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        zip.start_file(name, options).map_err(map_zip_error)?;
        let mut file = fs::File::open(&path)?;
        std::io::copy(&mut file, zip).map_err(map_zip_io_error)?;
        files_zipped += 1;
    }
    Ok(files_zipped)
}

fn map_zip_error(error: zip::result::ZipError) -> JobError {
    match error {
        zip::result::ZipError::Io(error) => map_zip_io_error(error),
        other => JobError::Internal(other.into()),
    }
}

fn map_zip_io_error(error: io::Error) -> JobError {
    JobError::Internal(error.into())
}

pub(crate) fn infer_message_type(parsed: &swift_core::ParsedMessage<'_>) -> Option<String> {
    let block2 = parsed
        .blocks
        .iter()
        .find(|block| block.id == BlockId::ApplicationHeader)?;
    let content = std::str::from_utf8(block2.content).ok()?;
    if content.starts_with('I') && content.len() >= 4 {
        return Some(format!("MT{}", &content[1..4]));
    }
    if content.starts_with('O') && content.len() >= 4 {
        return Some(format!("MT{}", &content[1..4]));
    }
    None
}

pub(crate) fn load_catalog(paths: &[PathBuf]) -> Result<SchemaCatalog> {
    let mut catalog = SchemaCatalog {
        field_types: Vec::new(),
        messages: Vec::new(),
    };
    for path in schema_files(paths)? {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read schema {}", path.display()))?;
        let mut partial = SchemaCatalog::from_yaml_str(&content)
            .with_context(|| format!("failed to parse schema {}", path.display()))?;
        for field_type in partial.field_types.drain(..) {
            if let Some(existing) = catalog
                .field_types
                .iter()
                .find(|existing| existing.name == field_type.name)
            {
                if existing != &field_type {
                    bail!(
                        "conflicting duplicate field type '{}' in {}",
                        field_type.name,
                        path.display()
                    );
                }
                continue;
            }
            catalog.field_types.push(field_type);
        }
        catalog.messages.append(&mut partial.messages);
    }
    Ok(catalog)
}

fn schema_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for path in paths {
        if path.is_dir() {
            for entry in fs::read_dir(path)? {
                let path = entry?.path();
                if matches!(
                    path.extension().and_then(|ext| ext.to_str()),
                    Some("yaml" | "yml")
                ) {
                    files.push(path);
                }
            }
        } else {
            files.push(path.clone());
        }
    }
    files.sort();
    Ok(files)
}

pub(crate) fn new_job_id() -> String {
    Uuid::new_v4().to_string()
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().try_into().unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::io;
    use std::sync::{Arc, Mutex};
    use swift_db::{FieldRow, NormalizedRow, ParseErrorRow, RawMessageRow};
    use tracing::field::{Field, Visit};
    use tracing::span::Record;
    use tracing::{Id, Subscriber};
    use tracing_subscriber::layer::{Context, SubscriberExt};
    use tracing_subscriber::Layer;

    const SCHEMA: &str = r#"
field_types:
  - name: reference
    pattern:
      - kind: literal
        value: ":"
      - kind: capture
        name: qualifier
        value_type: alpha_num
        length: 4
      - kind: literal
        value: "//"
      - kind: rest
        name: value
messages:
  - message: MT540
    sequences:
      GENL: {}
    fields:
      - path: GENL
        tag: 20C
        qualifier: SEME
        name: sender_reference
        type: reference
        required: true
        entity: settlement_instruction
        column: sender_reference
"#;

    #[derive(Debug, Clone)]
    struct CapturedSpan {
        id: u64,
        name: String,
        fields: BTreeMap<String, String>,
    }

    #[derive(Debug, Clone)]
    struct CapturedEvent {
        fields: BTreeMap<String, String>,
    }

    #[derive(Debug)]
    struct CaptureLayer {
        spans: Arc<Mutex<Vec<CapturedSpan>>>,
        events: Arc<Mutex<Vec<CapturedEvent>>>,
    }

    #[derive(Debug, Default)]
    struct CapturingSink {
        rows_seen: usize,
    }

    #[derive(Debug)]
    struct FakeParquetExporter {
        files: Vec<(&'static str, &'static [u8])>,
    }

    #[derive(Debug)]
    struct RecordingSystemOfRecordSink {
        operations: Arc<Mutex<Vec<&'static str>>>,
    }

    impl ParsedSink for CapturingSink {
        type Error = io::Error;

        fn write_batch(
            &mut self,
            batch: &ParsedOutputBatch,
        ) -> std::result::Result<(), Self::Error> {
            self.rows_seen = parsed_output_row_count(batch);
            Ok(())
        }
    }

    impl ParquetExporter for FakeParquetExporter {
        type Error = io::Error;

        fn export_parquet_layout(
            &self,
            _layout: &DatabaseLayout,
            directory: &Path,
        ) -> std::result::Result<Vec<ExportedTable>, Self::Error> {
            fs::create_dir_all(directory)?;
            self.files
                .iter()
                .map(|(name, bytes)| {
                    let path = directory.join(name);
                    fs::write(&path, bytes)?;
                    Ok(ExportedTable {
                        table: name.trim_end_matches(".parquet").to_string(),
                        path,
                        format: DuckDbExportFormat::Parquet,
                    })
                })
                .collect()
        }
    }

    impl SystemOfRecordSink for RecordingSystemOfRecordSink {
        fn upsert_job(&mut self, _job: &JobRecord) -> Result<()> {
            self.operations
                .lock()
                .expect("operations poisoned")
                .push("upsert_job");
            Ok(())
        }

        fn append_audit_event(&mut self, _event: &AuditEvent) -> Result<()> {
            self.operations
                .lock()
                .expect("operations poisoned")
                .push("append_audit_event");
            Ok(())
        }

        fn mark_artifact_committed(&mut self, _artifact: &ArtifactRecord) -> Result<()> {
            self.operations
                .lock()
                .expect("operations poisoned")
                .push("mark_artifact_committed");
            Ok(())
        }
    }

    impl<S> Layer<S> for CaptureLayer
    where
        S: Subscriber,
    {
        fn on_new_span(
            &self,
            attrs: &tracing::span::Attributes<'_>,
            id: &Id,
            _ctx: Context<'_, S>,
        ) {
            let mut visitor = FieldVisitor::default();
            attrs.record(&mut visitor);
            self.spans
                .lock()
                .expect("span capture poisoned")
                .push(CapturedSpan {
                    id: id.into_u64(),
                    name: attrs.metadata().name().to_string(),
                    fields: visitor.fields,
                });
        }

        fn on_record(&self, id: &Id, values: &Record<'_>, _ctx: Context<'_, S>) {
            let mut visitor = FieldVisitor::default();
            values.record(&mut visitor);
            let mut spans = self.spans.lock().expect("span capture poisoned");
            let Some(span) = spans.iter_mut().find(|span| span.id == id.into_u64()) else {
                return;
            };
            span.fields.extend(visitor.fields);
        }

        fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
            let mut visitor = FieldVisitor::default();
            event.record(&mut visitor);
            self.events
                .lock()
                .expect("event capture poisoned")
                .push(CapturedEvent {
                    fields: visitor.fields,
                });
        }
    }

    #[derive(Debug, Default)]
    struct FieldVisitor {
        fields: BTreeMap<String, String>,
    }

    impl Visit for FieldVisitor {
        fn record_str(&mut self, field: &Field, value: &str) {
            self.fields
                .insert(field.name().to_string(), value.to_string());
        }

        fn record_u64(&mut self, field: &Field, value: u64) {
            self.fields
                .insert(field.name().to_string(), value.to_string());
        }

        fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
            self.fields
                .insert(field.name().to_string(), format!("{value:?}"));
        }
    }

    fn capture_spans(run: impl FnOnce()) -> Vec<CapturedSpan> {
        let spans = Arc::new(Mutex::new(Vec::new()));
        let events = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::registry().with(CaptureLayer {
            spans: Arc::clone(&spans),
            events,
        });

        tracing::subscriber::with_default(subscriber, run);

        let captured = spans.lock().expect("span capture poisoned").clone();
        captured
    }

    fn capture_events(run: impl FnOnce()) -> Vec<CapturedEvent> {
        let spans = Arc::new(Mutex::new(Vec::new()));
        let events = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::registry().with(CaptureLayer {
            spans,
            events: Arc::clone(&events),
        });

        tracing::subscriber::with_default(subscriber, run);

        let captured = events.lock().expect("event capture poisoned").clone();
        captured
    }

    #[test]
    fn job_event_emits_structured_lifecycle_event() {
        let manifest = JobManifest {
            contract_version: CONTRACT_VERSION,
            job_id: "job-1".to_string(),
            status: "completed".to_string(),
            processing_elapsed_ms: 42,
            timings: JobTimings::default(),
            input_uris: vec!["s3://swiftpipe-inbox/input.fin".to_string()],
            output_prefix: "s3://swiftpipe-outbox/jobs/job-1/".to_string(),
            outputs: job_output_uris("s3://swiftpipe-outbox/jobs/job-1/"),
            counts: JobCounts {
                input_objects: 1,
                messages: 2,
                parse_errors: 3,
                rendered: 4,
            },
            messages: Vec::new(),
        };

        let events = capture_events(|| JobEvent::completed(&manifest, 5).emit());

        let event = events
            .iter()
            .find(|event| {
                event.fields.get("event").map(String::as_str) == Some("swiftpipe.job_event")
            })
            .expect("job lifecycle event emitted");
        assert_eq!(
            event.fields.get("job_id").map(String::as_str),
            Some("job-1")
        );
        assert_eq!(
            event.fields.get("status").map(String::as_str),
            Some("completed")
        );
        assert_eq!(
            event.fields.get("input_objects").map(String::as_str),
            Some("1")
        );
        assert_eq!(event.fields.get("messages").map(String::as_str), Some("2"));
        assert_eq!(
            event.fields.get("parse_errors").map(String::as_str),
            Some("3")
        );
        assert_eq!(event.fields.get("rendered").map(String::as_str), Some("4"));
        assert_eq!(
            event.fields.get("exported_tables").map(String::as_str),
            Some("5")
        );
        assert_eq!(
            event
                .fields
                .get("processing_elapsed_ms")
                .map(String::as_str),
            Some("42")
        );
    }

    #[test]
    fn parse_input_message_emits_structured_parse_span() {
        let input = b"{1:F01BANKBEBBAXXX0000000000}{2:I540BANKDEFFXXXXN}{4:\n:20C::SEME//ABC\n-}";

        let spans = capture_spans(|| {
            let parsed = parse_input_message(input, "s3://swiftpipe-inbox/input.fin", "msg-1");
            assert!(!parsed.blocks.is_empty());
        });

        let span = spans
            .iter()
            .find(|span| span.name == "swiftpipe.parse_message")
            .expect("parse span emitted");
        assert_eq!(
            span.fields.get("input_uri").map(String::as_str),
            Some("s3://swiftpipe-inbox/input.fin")
        );
        assert_eq!(
            span.fields.get("message_id").map(String::as_str),
            Some("msg-1")
        );
        let expected_input_bytes = input.len().to_string();
        assert_eq!(
            span.fields.get("input_bytes").map(String::as_str),
            Some(expected_input_bytes.as_str())
        );
    }

    #[test]
    fn materialize_input_message_emits_structured_schema_span() {
        let catalog = SchemaCatalog::from_yaml_str(SCHEMA).expect("schema loads");
        catalog.validate().expect("schema validates");
        let input = b"{1:F01BANKBEBBAXXX0000000000}{2:I540BANKDEFFXXXXN}{4:\n:16R:GENL\n:20C::SEME//ABC123\n:16S:GENL\n-}";
        let parsed = parse_message(input);
        let inbound = InboundMessage {
            id: "msg-1".to_string(),
            message_type: "MT540".to_string(),
            body: String::from_utf8(input.to_vec()).expect("input is utf-8"),
        };

        let spans = capture_spans(|| {
            let batch = materialize_input_message(
                &catalog,
                &inbound,
                &parsed,
                "s3://swiftpipe-inbox/input.fin",
            )
            .expect("materializes");
            assert!(!batch.normalized_rows.is_empty());
        });

        let span = spans
            .iter()
            .find(|span| span.name == "swiftpipe.schema_materialize")
            .expect("schema materialize span emitted");
        assert_eq!(
            span.fields.get("input_uri").map(String::as_str),
            Some("s3://swiftpipe-inbox/input.fin")
        );
        assert_eq!(
            span.fields.get("message_id").map(String::as_str),
            Some("msg-1")
        );
        assert_eq!(
            span.fields.get("message_type").map(String::as_str),
            Some("MT540")
        );
        let expected_parsed_blocks = parsed.blocks.len().to_string();
        assert_eq!(
            span.fields.get("parsed_blocks").map(String::as_str),
            Some(expected_parsed_blocks.as_str())
        );
    }

    #[test]
    fn write_duckdb_batch_emits_structured_write_span() {
        let batch = ParsedOutputBatch {
            raw_messages: vec![RawMessageRow {
                message_id: "msg-1".to_string(),
                message_type: "MT540".to_string(),
                raw_text: "{4:\n:16R:GENL\n-}".to_string(),
            }],
            fields: vec![
                FieldRow {
                    message_id: "msg-1".to_string(),
                    sequence_path: Some("GENL[0]".to_string()),
                    tag: "16R".to_string(),
                    qualifier: None,
                    raw_value: "GENL".to_string(),
                },
                FieldRow {
                    message_id: "msg-1".to_string(),
                    sequence_path: Some("GENL[0]".to_string()),
                    tag: "20C".to_string(),
                    qualifier: Some("SEME".to_string()),
                    raw_value: ":SEME//ABC123".to_string(),
                },
            ],
            parse_errors: vec![ParseErrorRow {
                message_id: "msg-1".to_string(),
                error: "missing required field".to_string(),
            }],
            normalized_rows: vec![NormalizedRow {
                table: "settlement_instruction".to_string(),
                values: BTreeMap::from([
                    ("message_id".to_string(), "msg-1".to_string()),
                    ("sequence_path".to_string(), "GENL[0]".to_string()),
                    ("sender_reference".to_string(), "ABC123".to_string()),
                ]),
            }],
        };
        let expected_rows_written = parsed_output_row_count(&batch).to_string();
        let metrics = Metrics::default();
        let mut sink = CapturingSink::default();

        let spans = capture_spans(|| {
            write_duckdb_batch(&mut sink, &batch, "job-1", &metrics).expect("writes batch");
        });

        assert_eq!(sink.rows_seen, 5);
        assert_eq!(metrics.duckdb_rows_written.get(), 5);
        let span = spans
            .iter()
            .find(|span| span.name == "swiftpipe.duckdb_write")
            .expect("duckdb write span emitted");
        assert_eq!(span.fields.get("job_id").map(String::as_str), Some("job-1"));
        assert_eq!(
            span.fields.get("rows_written").map(String::as_str),
            Some(expected_rows_written.as_str())
        );
        assert_eq!(
            span.fields.get("raw_messages").map(String::as_str),
            Some("1")
        );
        assert_eq!(span.fields.get("fields").map(String::as_str), Some("2"));
        assert_eq!(
            span.fields.get("parse_errors").map(String::as_str),
            Some("1")
        );
        assert_eq!(
            span.fields.get("normalized_rows").map(String::as_str),
            Some("1")
        );
    }

    #[test]
    fn export_parquet_tables_emits_structured_export_span() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let parquet_dir = temp_dir.path().join("parquet");
        let files = vec![
            (
                "settlement_instruction.parquet",
                b"first parquet bytes".as_slice(),
            ),
            ("swift_fields.parquet", b"second parquet bytes".as_slice()),
        ];
        let expected_bytes_written = files
            .iter()
            .map(|(_, bytes)| bytes.len() as u64)
            .sum::<u64>()
            .to_string();
        let exporter = FakeParquetExporter { files };
        let layout = DatabaseLayout { tables: Vec::new() };

        let spans = capture_spans(|| {
            let (tables_exported, bytes_written) =
                export_parquet_tables(&exporter, &layout, &parquet_dir, "job-1")
                    .expect("exports parquet");
            assert_eq!(tables_exported, 2);
            assert_eq!(bytes_written.to_string(), expected_bytes_written);
        });

        let expected_output_dir = parquet_dir.display().to_string();
        let span = spans
            .iter()
            .find(|span| span.name == "swiftpipe.parquet_export")
            .expect("parquet export span emitted");
        assert_eq!(span.fields.get("job_id").map(String::as_str), Some("job-1"));
        assert_eq!(
            span.fields.get("output_dir").map(String::as_str),
            Some(expected_output_dir.as_str())
        );
        assert_eq!(
            span.fields.get("tables_exported").map(String::as_str),
            Some("2")
        );
        assert_eq!(
            span.fields.get("bytes_written").map(String::as_str),
            Some(expected_bytes_written.as_str())
        );
    }

    #[test]
    fn write_zip_for_prefix_emits_structured_zip_span() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let store = LocalObjectStore::new(temp_dir.path());
        let prefix_uri = "s3://swiftpipe-outbox/jobs/job-1/";
        let zip_uri = "s3://swiftpipe-outbox/jobs/job-1/artifacts.zip";
        store
            .put_atomic(&format!("{prefix_uri}manifest.json"), br#"{"ok":true}"#)
            .expect("writes manifest");
        store
            .put_atomic(&format!("{prefix_uri}rendered/msg-1.fin"), b"{4:\n-}")
            .expect("writes rendered");

        let spans = capture_spans(|| {
            write_zip_for_prefix(
                &store,
                prefix_uri,
                zip_uri,
                ZipLimits {
                    max_total_bytes: crate::state::DEFAULT_ZIP_MAX_TOTAL_BYTES,
                    max_entries: crate::state::DEFAULT_ZIP_MAX_ENTRIES,
                },
            )
            .expect("writes zip");
        });

        let expected_bytes_written =
            fs::metadata(store.local_path_for_export(zip_uri).expect("zip path"))
                .expect("zip metadata")
                .len()
                .to_string();
        let span = spans
            .iter()
            .find(|span| span.name == "swiftpipe.zip_create")
            .expect("zip create span emitted");
        assert_eq!(
            span.fields.get("prefix_uri").map(String::as_str),
            Some(prefix_uri)
        );
        assert_eq!(
            span.fields.get("zip_uri").map(String::as_str),
            Some(zip_uri)
        );
        assert_eq!(
            span.fields.get("files_zipped").map(String::as_str),
            Some("2")
        );
        assert_eq!(
            span.fields.get("bytes_written").map(String::as_str),
            Some(expected_bytes_written.as_str())
        );
    }

    #[test]
    fn write_zip_for_prefix_rejects_entry_count_over_limit() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let store = LocalObjectStore::new(temp_dir.path());
        let prefix_uri = "s3://swiftpipe-outbox/jobs/job-entries/";
        let zip_uri = "s3://swiftpipe-outbox/jobs/job-entries/artifacts.zip";
        store
            .put_atomic(&format!("{prefix_uri}manifest.json"), br#"{"ok":true}"#)
            .expect("writes manifest");
        store
            .put_atomic(&format!("{prefix_uri}status.json"), br#"{"status":"ok"}"#)
            .expect("writes status");

        let events = capture_events(|| {
            let error = write_zip_for_prefix(
                &store,
                prefix_uri,
                zip_uri,
                ZipLimits {
                    max_total_bytes: crate::state::DEFAULT_ZIP_MAX_TOTAL_BYTES,
                    max_entries: 1,
                },
            )
            .expect_err("entry cap should reject zip");
            assert!(matches!(error, JobError::ZipLimitExceeded(_)));
            assert!(error.to_string().contains("max_zip_entries 1"));
        });

        assert!(events.iter().any(|event| {
            event.fields.get("event").map(String::as_str) == Some("swiftpipe.zip_limit_exceeded")
        }));
    }

    #[test]
    fn write_zip_for_prefix_rejects_total_bytes_over_limit() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let store = LocalObjectStore::new(temp_dir.path());
        let prefix_uri = "s3://swiftpipe-outbox/jobs/job-bytes/";
        let zip_uri = "s3://swiftpipe-outbox/jobs/job-bytes/artifacts.zip";
        store
            .put_atomic(&format!("{prefix_uri}manifest.json"), br#"{"ok":true}"#)
            .expect("writes manifest");

        let events = capture_events(|| {
            let error = write_zip_for_prefix(
                &store,
                prefix_uri,
                zip_uri,
                ZipLimits {
                    max_total_bytes: 1,
                    max_entries: crate::state::DEFAULT_ZIP_MAX_ENTRIES,
                },
            )
            .expect_err("byte cap should reject zip");
            assert!(matches!(error, JobError::ZipLimitExceeded(_)));
            assert!(error.to_string().contains("max_zip_total_bytes 1"));
        });

        assert!(events.iter().any(|event| {
            event.fields.get("event").map(String::as_str) == Some("swiftpipe.zip_limit_exceeded")
        }));
    }

    #[test]
    fn traced_system_record_sink_emits_structured_write_spans() {
        let operations = Arc::new(Mutex::new(Vec::new()));
        let mut sink = TracedSystemOfRecordSink {
            inner: Box::new(RecordingSystemOfRecordSink {
                operations: Arc::clone(&operations),
            }),
        };

        let spans = capture_spans(|| {
            sink.upsert_job(&JobRecord {
                job_id: "job-1".to_string(),
                status: "completed".to_string(),
                input_objects: 1,
                messages: 2,
                parse_errors: 0,
                rendered: 2,
                output_prefix: "s3://swiftpipe-outbox/jobs/job-1/".to_string(),
                processing_elapsed_ms: 42,
            })
            .expect("upserts job");
            sink.append_audit_event(&AuditEvent {
                job_id: "job-1".to_string(),
                event_type: "completed".to_string(),
                message: "done".to_string(),
            })
            .expect("appends audit event");
            sink.mark_artifact_committed(&ArtifactRecord {
                job_id: "job-1".to_string(),
                artifact_type: "manifest".to_string(),
                uri: "s3://swiftpipe-outbox/jobs/job-1/manifest.json".to_string(),
            })
            .expect("marks artifact");
        });

        assert_eq!(
            operations.lock().expect("operations poisoned").as_slice(),
            [
                "upsert_job",
                "append_audit_event",
                "mark_artifact_committed"
            ]
        );
        let system_spans = spans
            .iter()
            .filter(|span| span.name == "swiftpipe.system_record_write")
            .collect::<Vec<_>>();
        assert_eq!(system_spans.len(), 3);

        let upsert_span = system_spans
            .iter()
            .find(|span| span.fields.get("operation").map(String::as_str) == Some("upsert_job"))
            .expect("upsert span emitted");
        assert_eq!(
            upsert_span.fields.get("job_id").map(String::as_str),
            Some("job-1")
        );
        assert_eq!(
            upsert_span.fields.get("status").map(String::as_str),
            Some("completed")
        );
        assert_eq!(
            upsert_span.fields.get("messages").map(String::as_str),
            Some("2")
        );

        let audit_span = system_spans
            .iter()
            .find(|span| {
                span.fields.get("operation").map(String::as_str) == Some("append_audit_event")
            })
            .expect("audit span emitted");
        assert_eq!(
            audit_span.fields.get("event_type").map(String::as_str),
            Some("completed")
        );

        let artifact_span = system_spans
            .iter()
            .find(|span| {
                span.fields.get("operation").map(String::as_str) == Some("mark_artifact_committed")
            })
            .expect("artifact span emitted");
        assert_eq!(
            artifact_span
                .fields
                .get("artifact_type")
                .map(String::as_str),
            Some("manifest")
        );
        assert_eq!(
            artifact_span.fields.get("uri").map(String::as_str),
            Some("s3://swiftpipe-outbox/jobs/job-1/manifest.json")
        );
    }
}
