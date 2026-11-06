//! L1 of semantic ingress: the swiftpipe sink that lands `msg.*` tables in a
//! RAE engine (see `docs/design/semantic-ingress-contract.md` in qw).
//!
//! A file becomes messages ([`split`]), each message becomes `msg.raw`,
//! `msg.field` (generic, every MT type, [`fields`]) and the typed rows the
//! existing `ingest-core` adapters produce. Tables are posted as strict Parquet
//! with an `X-RAE-Source-Position` header so a re-send of the same file writes
//! nothing ([`identity`]).

pub mod client;
pub mod fields;
pub mod identity;
pub mod split;
pub mod tables;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use ingest_core::{CashStatement, SecurityEvent};
use swift_schema::SchemaCatalog;

pub use client::{HttpPoster, Outcome, Poster};
pub use identity::SourceId;
pub use split::{detect_format, Format, Kind};

use fields::{block4_matches_fields, derive_fields, envelope, sender_ref, FieldRow};
use identity::sha256_hex;
use split::Msg;
use tables::RawRow;

pub struct Options {
    pub source: SourceId,
    pub schema_dir: PathBuf,
    pub received_at_micros: i64,
    /// Messages per POST (per table).
    pub chunk_messages: usize,
    /// Also POST each message's bytes to `/v1/artifacts` (the contract keeps
    /// this off in P0: `rae-serve` has the artifact store disabled).
    pub artifacts: bool,
}

impl Options {
    pub fn new(source: SourceId, schema_dir: impl Into<PathBuf>) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_micros() as i64)
            .unwrap_or(0);
        Self {
            source,
            schema_dir: schema_dir.into(),
            received_at_micros: now,
            chunk_messages: 2000,
            artifacts: false,
        }
    }
}

/// Everything one message contributes to the tables.
#[derive(Debug, Clone)]
pub struct Landed {
    pub offset: usize,
    pub raw: RawRow,
    pub fields: Vec<FieldRow>,
    pub events: Vec<SecurityEvent>,
    pub statement: Option<CashStatement>,
    /// Block 4 does not equal its `msg.field` rows (field-level round trip).
    pub block4_mismatch: bool,
}

#[derive(Debug, Default)]
pub struct Prepared {
    pub file_sha256: String,
    pub srcns: String,
    pub landed: Vec<Landed>,
    pub skipped_fragments: usize,
    /// MT types with no catalog in the schema dir: fields land, typed rows do not.
    pub no_schema: BTreeSet<String>,
}

/// The MT schema catalogs, by message type (`MT513`). Loaded lazily from the
/// schema dir, or all at once with [`Schemas::preload`] (a long-lived worker).
pub struct Schemas {
    dir: PathBuf,
    cache: HashMap<String, Option<SchemaCatalog>>,
}

impl Schemas {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into(), cache: HashMap::new() }
    }

    /// Load every `mt<nnn>.yaml` in `dir` now; a type with no file still
    /// falls back to the (absent) disk lookup.
    pub fn preload(dir: impl Into<PathBuf>) -> Result<Self> {
        let mut s = Self::new(dir);
        for e in std::fs::read_dir(&s.dir)? {
            let name = e?.file_name().to_string_lossy().to_string();
            let Some(stem) = name.strip_suffix(".yaml") else { continue };
            if stem.starts_with("mt") && stem[2..].chars().all(|c| c.is_ascii_digit()) {
                let mtype = stem.to_uppercase();
                let c = load_catalog(&s.dir, &mtype);
                s.cache.insert(mtype, c);
            }
        }
        Ok(s)
    }

    /// Types with a loaded, valid catalog.
    pub fn loaded(&self) -> usize {
        self.cache.values().filter(|c| c.is_some()).count()
    }

    fn get(&mut self, mtype: &str) -> Option<&SchemaCatalog> {
        self.cache
            .entry(mtype.to_string())
            .or_insert_with(|| load_catalog(&self.dir, mtype))
            .as_ref()
    }
}

fn load_catalog(dir: &Path, mtype: &str) -> Option<SchemaCatalog> {
    let yaml = std::fs::read_to_string(dir.join(format!("{}.yaml", mtype.to_lowercase()))).ok()?;
    let c = SchemaCatalog::from_yaml_str(&yaml).ok()?;
    c.validate().ok()?;
    Some(c)
}

/// Parse a file into message rows; nothing is sent.
pub fn prepare(opts: &Options, file_name: &str, hint: Option<Format>, bytes: &[u8]) -> Result<Prepared> {
    prepare_with(opts, &mut Schemas::new(opts.schema_dir.clone()), file_name, hint, bytes, &|| false)
}

/// The text of the error [`prepare_with`] returns when `stop` turned true.
pub const CANCELLED: &str = "cancelled";

/// [`prepare`] against a caller-held [`Schemas`]; `stop` is checked between
/// messages and turns the parse into a [`CANCELLED`] error.
pub fn prepare_with(
    opts: &Options,
    schemas: &mut Schemas,
    file_name: &str,
    hint: Option<Format>,
    bytes: &[u8],
    stop: &dyn Fn() -> bool,
) -> Result<Prepared> {
    let format = match hint {
        Some(f) => f,
        None => detect_format(file_name, bytes)?,
    };
    let split = split::split_file(format, bytes)?;
    let file_sha = sha256_hex(bytes);
    let whole_text = std::str::from_utf8(bytes)?;
    let camt_statements = if format == Format::Camt {
        mx_camt::parse_camt053(whole_text, "")
    } else {
        Vec::new()
    };
    if format == Format::Camt && camt_statements.len() != split.messages.len() {
        return Err(anyhow!(
            "camt Stmt split ({}) disagrees with mx-camt ({})",
            split.messages.len(),
            camt_statements.len()
        ));
    }
    let mut out = Prepared {
        srcns: opts.source.srcns(&file_sha),
        file_sha256: file_sha.clone(),
        skipped_fragments: split.skipped_fragments,
        ..Default::default()
    };
    for m in &split.messages {
        if stop() {
            return Err(anyhow!(CANCELLED));
        }
        let id = opts.source.message_id(&file_sha, m.offset);
        let l = land_message(opts, schemas, &mut out.no_schema, &file_sha, file_name, &id, m, &camt_statements)?;
        out.landed.push(l);
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn land_message(
    opts: &Options,
    schemas: &mut Schemas,
    no_schema: &mut BTreeSet<String>,
    file_sha: &str,
    file_name: &str,
    id: &str,
    m: &Msg,
    camt: &[CashStatement],
) -> Result<Landed> {
    let src = opts.source.id.clone();
    let mut env = fields::Envelope::default();
    let mut rows: Vec<FieldRow> = Vec::new();
    let mut diags: Vec<String> = Vec::new();
    let mut mismatch = false;
    let mut events: Vec<SecurityEvent> = Vec::new();
    let mut statement: Option<CashStatement> = None;
    let mut ref_override: Option<String> = None;

    match &m.kind {
        Kind::Fin | Kind::FinBare => {
            let catalog = schemas.get(&m.message_type);
            if catalog.is_none() {
                no_schema.insert(m.message_type.clone());
            }
            let schema = catalog.and_then(|c| c.message(&m.message_type));
            let anchored = schema.map(swift_schema::anchored_sequences).unwrap_or_default();
            let parsed = swift_core::parse_message_with_sequences(m.parse_text.as_bytes(), &anchored);
            rows = derive_fields(&parsed);
            diags = parsed.diagnostics.iter().map(|d| format!("{d:?}")).collect();
            mismatch = !block4_matches_fields(&parsed, &rows);
            if m.kind == Kind::Fin {
                env = envelope(&parsed);
            }
            if m.message_type == "MT940" || m.message_type == "MT942" {
                statement = swift_mt940::parse_mt940(&m.parse_text, id);
                if let Some(s) = statement.as_mut() {
                    s.message_type = m.message_type.clone();
                }
            } else if let Some(cat) = catalog {
                let inbound = swift_db::InboundMessage {
                    id: id.to_string(),
                    message_type: m.message_type.clone(),
                    body: m.parse_text.clone(),
                };
                let plain = swift_core::parse_message(m.parse_text.as_bytes());
                if let Ok(batch) = swift_db::materialize_message(cat, &inbound, &plain) {
                    events = swift_normalize::normalize(&batch);
                }
            }
        }
        Kind::Camt { index } => {
            statement = camt.get(*index).cloned();
            let doc = roxmltree::Document::parse(&m.raw_text)?;
            ref_override = doc
                .root_element()
                .children()
                .find(|n| n.is_element() && n.tag_name().name() == "Id")
                .and_then(|n| n.text())
                .map(|t| t.trim().to_string());
        }
        Kind::Csv { header } => {
            let cols: Vec<String> = header.split(',').map(|c| c.trim().to_ascii_lowercase()).collect();
            for (i, cell) in m.raw_text.split(',').enumerate().take(cols.len()) {
                rows.push(FieldRow {
                    field_seq: i as i64,
                    seq_path: "$".into(),
                    seq_index: "$".into(),
                    tag: cols[i].clone(),
                    qualifier: None,
                    issuer: None,
                    value: cell.to_string(),
                    occurrence: 0,
                    raw: cell.to_string(),
                });
            }
            ref_override = rows
                .iter()
                .find(|r| r.tag == "id" && !r.value.trim().is_empty())
                .map(|r| r.value.trim().to_string());
            events = ingest_tabular::parse_csv(&format!("{header}\n{}", m.raw_text));
        }
    }
    for e in &mut events {
        e.source = src.clone();
        e.message_id = id.to_string();
    }
    if let Some(s) = statement.as_mut() {
        s.source = src.clone();
        s.message_id = id.to_string();
    }
    let raw = RawRow {
        source_id: src,
        message_id: id.to_string(),
        message_type: m.message_type.clone(),
        adapter: m.kind.adapter().to_string(),
        file_sha256: file_sha.to_string(),
        message_offset: m.offset as i64,
        message_sha256: sha256_hex(m.raw_text.as_bytes()),
        io: env.io,
        sender_bic: env.sender_bic,
        receiver_bic: env.receiver_bic,
        sender_ref: ref_override.or_else(|| sender_ref(&rows)),
        field_count: rows.len() as i64,
        diagnostic_count: diags.len() as i64,
        diagnostics: if diags.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&diags)?)
        },
        received_at_micros: opts.received_at_micros,
        file_name: Some(file_name.to_string()),
        raw_text: m.raw_text.clone(),
        artifact_ref: None,
    };
    Ok(Landed {
        offset: m.offset,
        raw,
        fields: rows,
        events,
        statement,
        block4_mismatch: mismatch,
    })
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TableCount {
    pub rows: usize,
    pub posts: usize,
    pub landed: usize,
    pub already_landed: usize,
}

#[derive(Debug, Default)]
pub struct FileReport {
    pub file: String,
    pub file_sha256: String,
    pub messages: usize,
    pub skipped_fragments: usize,
    /// message type -> (messages, msg.field rows)
    pub by_type: BTreeMap<String, (usize, usize)>,
    pub tables: BTreeMap<&'static str, TableCount>,
    pub diagnostic_messages: usize,
    pub block4_mismatches: usize,
    pub no_schema: BTreeSet<String>,
}

/// Post one prepared file. Order: every typed table and `msg.field`, then
/// `msg.raw` last (its trigger starts the flow that reads the others).
pub fn post(poster: &dyn Poster, opts: &Options, file: &str, p: &mut Prepared) -> Result<FileReport> {
    let mut rep = FileReport {
        file: file.to_string(),
        file_sha256: p.file_sha256.clone(),
        messages: p.landed.len(),
        skipped_fragments: p.skipped_fragments,
        no_schema: p.no_schema.clone(),
        ..Default::default()
    };
    if opts.artifacts {
        for l in &mut p.landed {
            l.raw.artifact_ref = Some(poster.post_artifact(l.raw.raw_text.as_bytes())?);
        }
    }
    for l in &p.landed {
        let e = rep.by_type.entry(l.raw.message_type.clone()).or_default();
        e.0 += 1;
        e.1 += l.fields.len();
        rep.diagnostic_messages += (l.raw.diagnostic_count > 0) as usize;
        rep.block4_mismatches += l.block4_mismatch as usize;
    }
    let chunk = opts.chunk_messages.max(1);
    let sid = &opts.source.id;
    let send = |rep: &mut FileReport, table: &'static str, batch: arrow::array::RecordBatch, pos: usize| -> Result<()> {
        let c = rep.tables.entry(table).or_default();
        c.rows += batch.num_rows();
        c.posts += 1;
        let position = format!("{}/{pos}", p.srcns);
        match poster.post(&table["msg.".len()..], tables::to_parquet(&batch)?, &position)? {
            Outcome::Landed => c.landed += 1,
            Outcome::AlreadyLanded => c.already_landed += 1,
        }
        Ok(())
    };
    for part in p.landed.chunks(chunk) {
        let pos = part.last().map(|l| l.offset).unwrap_or(0);
        for (table, batch) in typed_batches(sid, part)? {
            if batch.num_rows() > 0 {
                send(&mut rep, table, batch, pos)?;
            }
        }
    }
    for part in p.landed.chunks(chunk) {
        let pos = part.last().map(|l| l.offset).unwrap_or(0);
        let raws: Vec<RawRow> = part.iter().map(|l| l.raw.clone()).collect();
        send(&mut rep, "msg.raw", tables::raw_batch(&raws)?, pos)?;
    }
    Ok(rep)
}

/// The typed tables of some landed messages, in post order (`msg.field`,
/// `msg.security_event`, `msg.cash_statement`, `msg.cash_entry`); a table with
/// no rows is an empty batch of its contract schema. `msg.raw` is [`raw_batch`].
pub fn typed_batches(source_id: &str, landed: &[Landed]) -> Result<Vec<(&'static str, arrow::array::RecordBatch)>> {
    let frows: Vec<(&str, &str, &FieldRow)> = landed
        .iter()
        .flat_map(|l| l.fields.iter().map(move |f| (source_id, l.raw.message_id.as_str(), f)))
        .collect();
    let ev: Vec<(i64, &SecurityEvent)> = landed
        .iter()
        .flat_map(|l| l.events.iter().enumerate().map(|(i, e)| (i as i64, e)))
        .collect();
    let sts: Vec<&CashStatement> = landed.iter().filter_map(|l| l.statement.as_ref()).collect();
    Ok(vec![
        ("msg.field", tables::field_batch(&frows)?),
        ("msg.security_event", tables::security_event_batch(&ev)?),
        ("msg.cash_statement", tables::cash_statement_batch(&sts)?),
        ("msg.cash_entry", tables::cash_entry_batch(&sts)?),
    ])
}

/// The `msg.raw` rows of some landed messages.
pub fn raw_batch(landed: &[Landed]) -> Result<arrow::array::RecordBatch> {
    let raws: Vec<RawRow> = landed.iter().map(|l| l.raw.clone()).collect();
    tables::raw_batch(&raws)
}

/// Prepare and post one file.
pub fn ingest_file(
    poster: &dyn Poster,
    opts: &Options,
    path_name: &str,
    hint: Option<Format>,
    bytes: &[u8],
) -> Result<FileReport> {
    let base = path_name.rsplit('/').next().unwrap_or(path_name);
    let mut p = prepare(opts, base, hint, bytes)?;
    post(poster, opts, path_name, &mut p)
}
