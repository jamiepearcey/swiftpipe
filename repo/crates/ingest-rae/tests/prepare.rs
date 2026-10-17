//! Parse-only checks over the real fixtures (no network).

use ingest_rae::{prepare, Options, SourceId};

fn ex() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn opts(src: &str) -> Options {
    Options::new(SourceId::parse(src).unwrap(), ex().join("schemas"))
}

#[test]
fn mt940_rows_and_recon() {
    let bytes = std::fs::read(ex().join("mt940_sample.fin")).unwrap();
    let p = prepare(&opts("custodian:BANKGB22XXX"), "mt940_sample.fin", None, &bytes).unwrap();
    assert_eq!(p.landed.len(), 1);
    let l = &p.landed[0];
    assert_eq!(l.raw.message_type, "MT940");
    assert_eq!(l.raw.adapter, "swift");
    assert_eq!(l.raw.sender_ref.as_deref(), Some("STMT260513"));
    assert_eq!(l.raw.sender_bic.as_deref(), Some("BANKGB22XXX"));
    assert_eq!(l.raw.field_count as usize, l.fields.len());
    assert!(!l.block4_mismatch);
    // MT940 anchored frames: the :61: rows carry ENTRY[n]
    let entries: Vec<&str> = l.fields.iter().filter(|f| f.tag == "61").map(|f| f.seq_index.as_str()).collect();
    assert_eq!(entries, vec!["ENTRY[0]", "ENTRY[1]"]);
    let st = l.statement.as_ref().unwrap();
    assert_eq!(st.source, "custodian:BANKGB22XXX");
    assert_eq!(st.entries.len(), 2);
    assert_eq!(st.reconciles(), Some(true));
}

#[test]
fn the_break_fixture_does_not_reconcile() {
    let bytes = std::fs::read(ex().join("mt940_break_sample.fin")).unwrap();
    let p = prepare(&opts("custodian:BANKUS33XXX"), "mt940_break_sample.fin", None, &bytes).unwrap();
    assert_eq!(p.landed[0].statement.as_ref().unwrap().reconciles(), Some(false));
}

#[test]
fn csv_rows_and_camt_statement() {
    let bytes = std::fs::read(ex().join("positions_sample.csv")).unwrap();
    let p = prepare(&opts("broker:sample"), "positions_sample.csv", None, &bytes).unwrap();
    assert_eq!(p.landed.len(), String::from_utf8_lossy(&bytes).lines().filter(|l| !l.trim().is_empty()).count() - 1);
    assert_eq!(p.landed[0].raw.adapter, "tabular");
    assert_eq!(p.landed[0].offset, "isin,quantity,account,settlement_date,party,description\n".len());
    assert_eq!(p.landed[0].events.len(), 1);
    assert_eq!(p.landed[0].fields[0].tag, "isin");
    assert_eq!(p.landed[0].events[0].source, "broker:sample");

    let bytes = std::fs::read(ex().join("camt053_sample.xml")).unwrap();
    let p = prepare(&opts("custodian:BANKGB22XXX"), "camt053_sample.xml", None, &bytes).unwrap();
    assert!(!p.landed.is_empty());
    assert_eq!(p.landed[0].raw.adapter, "mx");
    assert_eq!(p.landed[0].raw.message_type, "camt.053");
    assert_eq!(p.landed[0].raw.field_count, 0);
    assert!(p.landed[0].statement.is_some());
}

#[test]
fn penalty_statement_csv_is_refused() {
    let bytes = std::fs::read(ex().join("mt537_penalty_statement.csv")).unwrap();
    let e = prepare(&opts("broker:sample"), "mt537_penalty_statement.csv", None, &bytes).unwrap_err();
    assert!(format!("{e:#}").contains("unknown_layout"), "{e:#}");
}

#[test]
fn ids_are_stable_across_runs() {
    let bytes = std::fs::read(ex().join("mt940_sample.fin")).unwrap();
    let a = prepare(&opts("custodian:BANKGB22XXX"), "a.fin", None, &bytes).unwrap();
    let b = prepare(&opts("custodian:BANKGB22XXX"), "renamed.fin", None, &bytes).unwrap();
    assert_eq!(a.landed[0].raw.message_id, b.landed[0].raw.message_id);
    assert_eq!(a.srcns, b.srcns);
}
