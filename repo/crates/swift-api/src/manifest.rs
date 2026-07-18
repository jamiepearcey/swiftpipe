use serde::{Deserialize, Serialize};

pub(crate) const CONTRACT_VERSION: &str = "1";
#[allow(dead_code)]
pub(crate) const JOB_REQUEST_FIELDS: &[&str] = &[
    "input_uri",
    "input_prefix",
    "output_prefix",
    "message_type",
    "include_suffix",
    "render_validate",
    "outputs",
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JobRequest {
    pub(crate) input_uri: Option<String>,
    pub(crate) input_prefix: Option<String>,
    pub(crate) output_prefix: Option<String>,
    pub(crate) message_type: Option<String>,
    pub(crate) include_suffix: Option<String>,
    pub(crate) render_validate: Option<bool>,
    pub(crate) outputs: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub(crate) struct JobManifest {
    pub(crate) contract_version: &'static str,
    pub(crate) job_id: String,
    pub(crate) status: String,
    pub(crate) processing_elapsed_ms: u64,
    pub(crate) timings: JobTimings,
    pub(crate) input_uris: Vec<String>,
    pub(crate) output_prefix: String,
    pub(crate) outputs: JobOutputs,
    pub(crate) counts: JobCounts,
    pub(crate) messages: Vec<MessageResult>,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct JobTimings {
    pub(crate) setup_ms: u64,
    pub(crate) prepare_ms: u64,
    pub(crate) hydrate_write_ms: u64,
    pub(crate) export_ms: u64,
    pub(crate) manifest_ms: u64,
    pub(crate) zip_ms: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct JobStatusArtifact {
    pub(crate) job_id: String,
    pub(crate) status: String,
    pub(crate) manifest_uri: String,
    pub(crate) error: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct JobOutputs {
    pub(crate) manifest: String,
    pub(crate) normalized_parquet_prefix: String,
    pub(crate) errors_ndjson: String,
    pub(crate) rendered_prefix: String,
    pub(crate) zip: String,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct JobCounts {
    pub(crate) input_objects: usize,
    pub(crate) messages: usize,
    pub(crate) parse_errors: usize,
    pub(crate) rendered: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct MessageResult {
    pub(crate) message_id: String,
    pub(crate) message_type: String,
    pub(crate) input_uri: String,
    pub(crate) status: String,
    pub(crate) elapsed_ms: u64,
    pub(crate) parse_errors: usize,
    pub(crate) rendered_uri: Option<String>,
    pub(crate) error: Option<String>,
}

pub(crate) fn job_output_uris(output_prefix: &str) -> JobOutputs {
    JobOutputs {
        manifest: format!("{output_prefix}manifest.json"),
        normalized_parquet_prefix: format!("{output_prefix}normalized/"),
        errors_ndjson: format!("{output_prefix}errors.ndjson"),
        rendered_prefix: format!("{output_prefix}rendered/"),
        zip: format!("{output_prefix}exports.zip"),
    }
}
