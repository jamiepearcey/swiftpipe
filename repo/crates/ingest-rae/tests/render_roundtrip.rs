//! Render round-trip gate: for every `.fin` fixture under `examples/`,
//! parse -> `msg.field` rows -> normalized rows -> `render_message` -> parse
//! must give identical `msg.field` rows. Fixtures that cannot are listed in
//! `tests/render_roundtrip.expected` with their class; the test fails when the
//! actual set differs, so a regression or an improvement is always visible.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ingest_rae::fields::{derive_fields, FieldRow};
use swift_core::BlockId;
use swift_schema::{RenderEnvelope, RenderRequest, RenderRow, SchemaCatalog};

fn ex() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn fin_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            if p.file_name().unwrap() != "schemas" {
                fin_files(&p, out);
            }
        } else if p.extension().map_or(false, |e| e == "fin") {
            out.push(p);
        }
    }
}

fn parse_rows(text: &str, catalog: &SchemaCatalog, mtype: &str) -> (Vec<FieldRow>, usize) {
    let anchored = catalog
        .message(mtype)
        .map(swift_schema::anchored_sequences)
        .unwrap_or_default();
    let parsed = swift_core::parse_message_with_sequences(text.as_bytes(), &anchored);
    (derive_fields(&parsed), parsed.diagnostics.len())
}

fn block(parsed: &swift_core::ParsedMessage<'_>, id: BlockId<'_>) -> Option<String> {
    parsed
        .blocks
        .iter()
        .find(|b| b.id == id)
        .map(|b| String::from_utf8_lossy(b.content).into_owned())
}

/// `ok`, or a class: `no_schema`, `render_error`, `diagnostics`, `differs(n_before->n_after)`.
fn round_trip(text: &str, mtype: &str, schema_dir: &Path) -> String {
    let yaml = match std::fs::read_to_string(schema_dir.join(format!("{}.yaml", mtype.to_lowercase()))) {
        Ok(y) => y,
        Err(_) => return "no_schema".into(),
    };
    let catalog = SchemaCatalog::from_yaml_str(&yaml).unwrap();
    let (before, diag) = parse_rows(text, &catalog, mtype);
    if diag > 0 {
        return format!("diagnostics({diag})");
    }
    let parsed = swift_core::parse_message(text.as_bytes());
    let inbound = swift_db::InboundMessage {
        id: "rt".into(),
        message_type: mtype.into(),
        body: text.into(),
    };
    let batch = match swift_db::materialize_message(&catalog, &inbound, &parsed) {
        Ok(b) => b,
        Err(e) => return format!("materialize_error({e})"),
    };
    let request = RenderRequest {
        message_id: "rt".into(),
        message_type: mtype.into(),
        envelope: RenderEnvelope {
            block1: block(&parsed, BlockId::BasicHeader).unwrap_or_default(),
            block2: block(&parsed, BlockId::ApplicationHeader).unwrap_or_default(),
            block3: block(&parsed, BlockId::UserHeader),
            block5: parsed
                .blocks
                .iter()
                .find(|b| b.id == BlockId::Other(b"5"))
                .map(|b| String::from_utf8_lossy(b.content).into_owned()),
        },
        rows: batch
            .normalized_rows
            .iter()
            .map(|r| RenderRow { table: r.table.clone(), values: r.values.clone() })
            .collect(),
    };
    let rendered = match swift_schema::render_message(&catalog, &request) {
        Ok(r) => r,
        Err(e) => {
            let s = format!("{e:?}");
            return format!("render_error({})", s.split(|c: char| !c.is_alphanumeric()).next().unwrap_or(""));
        }
    };
    let (after, diag2) = parse_rows(&rendered, &catalog, mtype);
    if diag2 > 0 {
        return format!("rendered_with_diagnostics({diag2})");
    }
    if before == after {
        "ok".into()
    } else {
        if std::env::var("DEBUG_RT").is_ok() {
            if let Some((b, a)) = before.iter().zip(after.iter()).find(|(b, a)| b != a) {
                println!("  first diff {mtype}: before {b:?}\n                after  {a:?}");
            }
        }
        format!("differs({}->{} rows)", before.len(), after.len())
    }
}

#[test]
fn render_round_trip_gate() {
    let mut files = Vec::new();
    fin_files(&ex(), &mut files);
    assert!(files.len() >= 69, "{} fixtures", files.len());
    let mut results: BTreeMap<String, String> = BTreeMap::new();
    for f in &files {
        let bytes = std::fs::read(f).unwrap();
        let rel = f.strip_prefix(ex()).unwrap().display().to_string();
        let msgs = swift_core::split_messages(&bytes);
        if msgs.is_empty() {
            results.insert(rel, "no_fin_message".into());
            continue;
        }
        for (i, m) in msgs.iter().enumerate() {
            let text = std::str::from_utf8(m).unwrap();
            let mtype = format!("MT{}", swift_core::message_type_of(m).unwrap_or("???"));
            let class = round_trip(text, &mtype, &ex().join("schemas"));
            results.insert(format!("{rel}#{i}"), class);
        }
    }
    let ok = results.values().filter(|c| *c == "ok").count();
    let mut report = String::new();
    for (k, v) in &results {
        if v != "ok" {
            report.push_str(&format!("{k}\t{v}\n"));
        }
    }
    println!("render round trip: {ok} ok of {} messages in {} fixtures", results.len(), files.len());
    println!("{report}");
    let expected_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/render_roundtrip.expected");
    if std::env::var("UPDATE_RENDER_EXPECTED").is_ok() {
        std::fs::write(&expected_path, &report).unwrap();
    }
    let expected = std::fs::read_to_string(&expected_path).unwrap_or_default();
    assert_eq!(report, expected, "the set of fixtures that cannot round-trip changed");
}
