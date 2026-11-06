//! The `swift_parse` step worker (`holon-http/1`, ADR-0052).
//!
//! The engine asks `POST /step/swift_parse` with `input.files` = a reference to an ingress inbox
//! snapshot (`{"$ref": {"ns","table","version"}}` or `{"$ref": "/v1/ns/table@vN"}`), one row per
//! file with its source id, name and bytes. The worker reads it with `GET
//! {HOLON_ENGINE}/v1/{ns}/{table}?version={v}` (Arrow IPC stream, `Bearer HOLON_TOKEN`), parses each
//! file with the `ingest-rae` sink code (split, `msg.field`, the typed adapters) and answers one
//! Arrow table per `msg.*` table: `msg.field`, `msg.security_event`, `msg.cash_statement`,
//! `msg.cash_entry` and `msg.raw` last, always all five (a table with no rows is an empty stream of
//! its contract schema). A file the sink refuses (not UTF-8, unknown format, `unknown_layout` CSV,
//! a bad source id) lands nothing and is listed in `output.refused` with the sink's reason; the
//! contract has no refusal table. Small inputs may stay inline: `input.files` = `[{source_id, name,
//! text}]`.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{anyhow, bail, Context, Result};
use arrow::array::{Array, BinaryArray, LargeBinaryArray, LargeStringArray, RecordBatch, StringArray};
use arrow::compute::concat_batches;
use rae_holon_worker::{StepReply, StepRequest};
use serde_json::{json, Value};

use ingest_rae::{prepare_with, Landed, Options, Schemas, SourceId, CANCELLED};

pub const ARROW_STREAM: &str = "application/vnd.apache.arrow.stream";

/// The reply tables, in reply order.
pub const TABLES: [&str; 5] = ["msg.field", "msg.security_event", "msg.cash_statement", "msg.cash_entry", "msg.raw"];

/// One inbox row.
#[derive(Debug, Clone)]
pub struct InFile {
    pub source_id: String,
    pub name: String,
    pub bytes: Vec<u8>,
}

/// A table version to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRef {
    pub ns: String,
    pub table: String,
    pub version: u64,
}

/// `{"ns","table","version"}` or the flow string form `/v1/ns/table@vN`.
pub fn parse_ref(v: &Value) -> Result<TableRef> {
    if let Some(s) = v.as_str() {
        let rest = s.strip_prefix("/v1/").ok_or_else(|| anyhow!("a $ref string is /v1/<ns>/<table>@v<N>, got {s}"))?;
        let (path, ver) = rest.rsplit_once("@v").ok_or_else(|| anyhow!("a $ref string names its version (@vN): {s}"))?;
        let (ns, table) = path.split_once('/').ok_or_else(|| anyhow!("a $ref string names ns and table: {s}"))?;
        return Ok(TableRef { ns: ns.into(), table: table.into(), version: ver.parse().with_context(|| format!("version in {s}"))? });
    }
    let text = |k: &str| v[k].as_str().map(str::to_string).ok_or_else(|| anyhow!("$ref.{k} is missing"));
    let version = v["version"].as_u64().or_else(|| v["version"].as_str().and_then(|s| s.trim_start_matches('v').parse().ok()));
    Ok(TableRef { ns: text("ns")?, table: text("table")?, version: version.ok_or_else(|| anyhow!("$ref.version is missing"))? })
}

/// Read a table version from the engine as Arrow.
pub fn fetch(engine: &str, token: &str, r: &TableRef) -> Result<Vec<RecordBatch>> {
    let url = format!("{}/v1/{}/{}?version={}", engine.trim_end_matches('/'), r.ns, r.table, r.version);
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).build().into();
    let mut resp = agent
        .get(&url)
        .header("Accept", ARROW_STREAM)
        .header("Authorization", format!("Bearer {token}"))
        .call()
        .map_err(|e| anyhow!("GET {url}: {e}"))?;
    let status = resp.status().as_u16();
    let body = resp.body_mut().with_config().limit(u64::MAX).read_to_vec().map_err(|e| anyhow!("GET {url}: {e}"))?;
    if status != 200 {
        bail!("GET {url}: {status} {}", String::from_utf8_lossy(&body[..body.len().min(400)]));
    }
    let reader = arrow::ipc::reader::StreamReader::try_new(std::io::Cursor::new(body), None)?;
    Ok(reader.collect::<std::result::Result<Vec<_>, _>>()?)
}

fn column<'a>(b: &'a RecordBatch, names: &[&str]) -> Option<&'a dyn Array> {
    names.iter().find_map(|n| b.column_by_name(n)).map(|c| c.as_ref())
}

fn cell(a: &dyn Array, i: usize) -> Option<Vec<u8>> {
    if a.is_null(i) {
        return None;
    }
    let any = a.as_any();
    if let Some(x) = any.downcast_ref::<BinaryArray>() {
        return Some(x.value(i).to_vec());
    }
    if let Some(x) = any.downcast_ref::<LargeBinaryArray>() {
        return Some(x.value(i).to_vec());
    }
    if let Some(x) = any.downcast_ref::<StringArray>() {
        return Some(x.value(i).as_bytes().to_vec());
    }
    any.downcast_ref::<LargeStringArray>().map(|x| x.value(i).as_bytes().to_vec())
}

/// Inbox rows: `source_id` (or `source`; else `default_source`), `name` (or `file_name`), `bytes`
/// (or `content`/`text`), binary or UTF-8.
pub fn inbox_rows(batches: &[RecordBatch], default_source: Option<&str>) -> Result<Vec<InFile>> {
    let mut out = Vec::new();
    for b in batches {
        let src = column(b, &["source_id", "source"]);
        let name = column(b, &["name", "file_name", "file", "path"]);
        let bytes = column(b, &["bytes", "content", "text", "body"]).ok_or_else(|| anyhow!("the inbox has no bytes column"))?;
        for i in 0..b.num_rows() {
            let text = |a: Option<&dyn Array>| a.and_then(|a| cell(a, i)).map(|v| String::from_utf8_lossy(&v).into_owned());
            out.push(InFile {
                source_id: text(src).or_else(|| default_source.map(str::to_string)).unwrap_or_default(),
                name: text(name).unwrap_or_else(|| format!("file-{}", out.len())),
                bytes: cell(bytes, i).unwrap_or_default(),
            });
        }
    }
    Ok(out)
}

/// The files of an ask: by reference (read from the engine) or inline.
pub fn input_files(input: &Value, engine: &str, token: &str) -> Result<Vec<InFile>> {
    let files = &input["files"];
    let default_source = input["source_id"].as_str();
    if let Some(r) = files.get("$ref") {
        let r = parse_ref(r)?;
        return inbox_rows(&fetch(engine, token, &r)?, default_source);
    }
    let list = files.as_array().ok_or_else(|| anyhow!("input.files is neither a $ref nor a list"))?;
    Ok(list
        .iter()
        .map(|f| InFile {
            source_id: f["source_id"].as_str().or(default_source).unwrap_or_default().to_string(),
            name: f["name"].as_str().unwrap_or_default().to_string(),
            bytes: f["text"].as_str().unwrap_or_default().as_bytes().to_vec(),
        })
        .collect())
}

pub fn arrow_stream(batch: &RecordBatch) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut w = arrow::ipc::writer::StreamWriter::try_new(&mut buf, &batch.schema())?;
    w.write(batch)?;
    w.finish()?;
    drop(w);
    Ok(buf)
}

/// What one ask parsed.
#[derive(Debug, Default)]
pub struct Parsed {
    /// One batch per [`TABLES`] entry, same order.
    pub tables: Vec<(&'static str, RecordBatch)>,
    pub output: Value,
}

/// The worker: the schema catalogs, loaded once.
pub struct Worker {
    schemas: Mutex<Schemas>,
    schema_dir: PathBuf,
}

impl Worker {
    pub fn new(schema_dir: impl Into<PathBuf>) -> Result<Self> {
        let schema_dir = schema_dir.into();
        let schemas = Schemas::preload(&schema_dir).with_context(|| format!("loading schemas from {}", schema_dir.display()))?;
        Ok(Self { schemas: Mutex::new(schemas), schema_dir })
    }

    pub fn schemas_loaded(&self) -> usize {
        self.schemas.lock().unwrap_or_else(std::sync::PoisonError::into_inner).loaded()
    }

    /// Parse files into the five tables; `stop` is checked between files and between messages.
    pub fn parse(&self, files: &[InFile], stop: &dyn Fn() -> bool) -> Result<Parsed> {
        let received = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_micros() as i64).unwrap_or(0);
        let mut schemas = self.schemas.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut parts: Vec<Vec<RecordBatch>> = vec![Vec::new(); TABLES.len()];
        let mut refused = Vec::new();
        let mut no_schema = BTreeSet::new();
        let (mut messages, mut diags, mut mismatches, mut skipped) = (0usize, 0usize, 0usize, 0usize);
        for f in files {
            if stop() {
                bail!(CANCELLED);
            }
            let source = match SourceId::parse(&f.source_id) {
                Ok(s) => s,
                Err(e) => {
                    refused.push(json!({"file": f.name, "source_id": f.source_id, "reason": format!("refused: source id: {e:#}")}));
                    continue;
                }
            };
            let mut opts = Options::new(source, self.schema_dir.clone());
            opts.received_at_micros = received;
            let base = f.name.rsplit('/').next().unwrap_or(&f.name);
            let p = match prepare_with(&opts, &mut schemas, base, None, &f.bytes, stop) {
                Ok(p) => p,
                Err(e) if e.to_string() == CANCELLED => bail!(CANCELLED),
                Err(e) => {
                    refused.push(json!({"file": f.name, "source_id": f.source_id, "reason": format!("{e:#}")}));
                    continue;
                }
            };
            let landed: &[Landed] = &p.landed;
            messages += landed.len();
            skipped += p.skipped_fragments;
            diags += landed.iter().filter(|l| l.raw.diagnostic_count > 0).count();
            mismatches += landed.iter().filter(|l| l.block4_mismatch).count();
            no_schema.extend(p.no_schema.iter().cloned());
            for (i, (_, b)) in ingest_rae::typed_batches(&opts.source.id, landed)?.into_iter().enumerate() {
                parts[i].push(b);
            }
            parts[4].push(ingest_rae::raw_batch(landed)?);
        }
        let empty = ingest_rae::typed_batches("", &[])?;
        let schema_of = |i: usize| if i < 4 { empty[i].1.schema() } else { ingest_rae::tables::raw_schema() };
        let mut tables = Vec::new();
        for (i, t) in TABLES.iter().enumerate() {
            let schema = schema_of(i);
            tables.push((*t, concat_batches(&schema, &parts[i])?));
        }
        let output = json!({
            "files": files.len(),
            "messages": messages,
            "refused": refused,
            "no_schema": no_schema,
            "skipped_fragments": skipped,
            "diagnostic_messages": diags,
            "block4_mismatches": mismatches,
        });
        Ok(Parsed { tables, output })
    }

    /// One `POST /step/{kind}` ask.
    pub fn handle(&self, req: &StepRequest, engine: &str, token: &str) -> StepReply {
        let files = match input_files(&req.input, engine, token) {
            Ok(f) => f,
            Err(e) => return StepReply::failed("input_unreadable", format!("{e:#}"), true),
        };
        let parsed = match self.parse(&files, &|| req.cancelled()) {
            Ok(p) => p,
            Err(e) if e.to_string() == CANCELLED => return StepReply::failed("cancelled", "the engine cancelled this attempt", false),
            Err(e) => return StepReply::failed("parse_failed", format!("{e:#}"), false),
        };
        let mut reply = StepReply::ok(parsed.output);
        for (t, b) in &parsed.tables {
            match arrow_stream(b) {
                Ok(bytes) => reply = reply.table(t, b.num_rows() as u64, bytes),
                Err(e) => return StepReply::failed("encode_failed", format!("{t}: {e:#}"), false),
            }
        }
        reply
    }
}
