//! Arrow batches for the `msg.*` tables. Schemas match the contract field for
//! field, including nullability: the sink posts with `?schema=strict`.

use std::sync::Arc;

use anyhow::Result;
use arrow::array::{
    ArrayRef, Int64Array, RecordBatch, StringArray, TimestampMicrosecondArray,
};
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use ingest_core::{CashStatement, SecurityEvent};
use parquet::arrow::ArrowWriter;

use crate::fields::FieldRow;

#[derive(Debug, Clone)]
pub struct RawRow {
    pub source_id: String,
    pub message_id: String,
    pub message_type: String,
    pub adapter: String,
    pub file_sha256: String,
    pub message_offset: i64,
    pub message_sha256: String,
    pub io: Option<String>,
    pub sender_bic: Option<String>,
    pub receiver_bic: Option<String>,
    pub sender_ref: Option<String>,
    pub field_count: i64,
    pub diagnostic_count: i64,
    pub diagnostics: Option<String>,
    pub received_at_micros: i64,
    pub file_name: Option<String>,
    pub raw_text: String,
    pub artifact_ref: Option<String>,
}

fn utf8(name: &str, nullable: bool) -> Field {
    Field::new(name, DataType::Utf8, nullable)
}
fn i64f(name: &str, nullable: bool) -> Field {
    Field::new(name, DataType::Int64, nullable)
}
fn req(v: impl Iterator<Item = String>) -> ArrayRef {
    Arc::new(StringArray::from_iter_values(v))
}
fn opt(v: impl Iterator<Item = Option<String>>) -> ArrayRef {
    Arc::new(StringArray::from_iter(v))
}
fn ints(v: impl Iterator<Item = i64>) -> ArrayRef {
    Arc::new(Int64Array::from_iter_values(v))
}

pub fn raw_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        utf8("source_id", false),
        utf8("message_id", false),
        utf8("message_type", false),
        utf8("adapter", false),
        utf8("file_sha256", false),
        i64f("message_offset", false),
        utf8("message_sha256", false),
        utf8("io", true),
        utf8("sender_bic", true),
        utf8("receiver_bic", true),
        utf8("sender_ref", true),
        i64f("field_count", false),
        i64f("diagnostic_count", false),
        utf8("diagnostics", true),
        Field::new(
            "received_at",
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            false,
        ),
        utf8("file_name", true),
        utf8("raw_text", false),
        utf8("artifact_ref", true),
    ]))
}

pub fn raw_batch(rows: &[RawRow]) -> Result<RecordBatch> {
    let cols: Vec<ArrayRef> = vec![
        req(rows.iter().map(|r| r.source_id.clone())),
        req(rows.iter().map(|r| r.message_id.clone())),
        req(rows.iter().map(|r| r.message_type.clone())),
        req(rows.iter().map(|r| r.adapter.clone())),
        req(rows.iter().map(|r| r.file_sha256.clone())),
        ints(rows.iter().map(|r| r.message_offset)),
        req(rows.iter().map(|r| r.message_sha256.clone())),
        opt(rows.iter().map(|r| r.io.clone())),
        opt(rows.iter().map(|r| r.sender_bic.clone())),
        opt(rows.iter().map(|r| r.receiver_bic.clone())),
        opt(rows.iter().map(|r| r.sender_ref.clone())),
        ints(rows.iter().map(|r| r.field_count)),
        ints(rows.iter().map(|r| r.diagnostic_count)),
        opt(rows.iter().map(|r| r.diagnostics.clone())),
        Arc::new(
            TimestampMicrosecondArray::from_iter_values(rows.iter().map(|r| r.received_at_micros))
                .with_timezone("UTC"),
        ),
        opt(rows.iter().map(|r| r.file_name.clone())),
        req(rows.iter().map(|r| r.raw_text.clone())),
        opt(rows.iter().map(|r| r.artifact_ref.clone())),
    ];
    Ok(RecordBatch::try_new(raw_schema(), cols)?)
}

pub fn field_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        utf8("source_id", false),
        utf8("message_id", false),
        i64f("field_seq", false),
        utf8("seq_path", false),
        utf8("seq_index", false),
        utf8("tag", false),
        utf8("qualifier", true),
        utf8("issuer", true),
        utf8("value", false),
        i64f("occurrence", false),
        utf8("raw", false),
    ]))
}

/// `(source_id, message_id, row)` triples, in file order.
pub fn field_batch(rows: &[(&str, &str, &FieldRow)]) -> Result<RecordBatch> {
    let cols: Vec<ArrayRef> = vec![
        req(rows.iter().map(|r| r.0.to_string())),
        req(rows.iter().map(|r| r.1.to_string())),
        ints(rows.iter().map(|r| r.2.field_seq)),
        req(rows.iter().map(|r| r.2.seq_path.clone())),
        req(rows.iter().map(|r| r.2.seq_index.clone())),
        req(rows.iter().map(|r| r.2.tag.clone())),
        opt(rows.iter().map(|r| r.2.qualifier.clone())),
        opt(rows.iter().map(|r| r.2.issuer.clone())),
        req(rows.iter().map(|r| r.2.value.clone())),
        ints(rows.iter().map(|r| r.2.occurrence)),
        req(rows.iter().map(|r| r.2.raw.clone())),
    ];
    Ok(RecordBatch::try_new(field_schema(), cols)?)
}

/// `source` -> `source_id`, and an optional trailing `row_seq:Int64!`.
fn contract_shape(batch: RecordBatch, row_seq: Option<Vec<i64>>) -> Result<RecordBatch> {
    let mut fields: Vec<Field> = batch
        .schema()
        .fields()
        .iter()
        .map(|f| f.as_ref().clone())
        .collect();
    fields[0] = Field::new("source_id", DataType::Utf8, false);
    let mut cols: Vec<ArrayRef> = batch.columns().to_vec();
    if let Some(seq) = row_seq {
        fields.push(i64f("row_seq", false));
        cols.push(ints(seq.into_iter()));
    }
    Ok(RecordBatch::try_new(Arc::new(Schema::new(fields)), cols)?)
}

/// `events` are `(row_seq, event)`; the ingest-parquet batch is reused as is.
pub fn security_event_batch(events: &[(i64, &SecurityEvent)]) -> Result<RecordBatch> {
    let owned: Vec<SecurityEvent> = events.iter().map(|e| e.1.clone()).collect();
    contract_shape(
        ingest_parquet::security_events_batch(&owned)?,
        Some(events.iter().map(|e| e.0).collect()),
    )
}

pub fn cash_statement_batch(statements: &[&CashStatement]) -> Result<RecordBatch> {
    let owned: Vec<CashStatement> = statements.iter().map(|s| (*s).clone()).collect();
    contract_shape(ingest_parquet::cash_statements_batch(&owned)?, None)
}

pub fn cash_entry_batch(statements: &[&CashStatement]) -> Result<RecordBatch> {
    let owned: Vec<CashStatement> = statements.iter().map(|s| (*s).clone()).collect();
    let seq: Vec<i64> = owned
        .iter()
        .flat_map(|s| (0..s.entries.len() as i64).collect::<Vec<_>>())
        .collect();
    contract_shape(ingest_parquet::cash_entries_batch(&owned)?, Some(seq))
}

pub fn to_parquet(batch: &RecordBatch) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut w = ArrowWriter::try_new(&mut buf, batch.schema(), None)?;
    w.write(batch)?;
    w.close()?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ingest_core::{CashEntry, Direction};

    fn names(b: &RecordBatch) -> Vec<(String, bool)> {
        b.schema()
            .fields()
            .iter()
            .map(|f| (f.name().clone(), f.is_nullable()))
            .collect()
    }

    #[test]
    fn typed_batches_follow_the_contract_columns() {
        let st = CashStatement {
            source: "custodian:BANKGB22XXX".into(),
            message_id: "m-1".into(),
            message_type: "MT940".into(),
            entries: vec![CashEntry::default(), CashEntry { direction: Direction::Debit, ..Default::default() }],
            ..Default::default()
        };
        let e = cash_entry_batch(&[&st]).unwrap();
        let n = names(&e);
        assert_eq!(n[0], ("source_id".into(), false));
        assert_eq!(n[13], ("row_seq".into(), false));
        assert_eq!(n.len(), 14);
        assert_eq!(e.num_rows(), 2);
        let seq = e.column(13).as_any().downcast_ref::<Int64Array>().unwrap();
        assert_eq!((seq.value(0), seq.value(1)), (0, 1));
        let s = cash_statement_batch(&[&st]).unwrap();
        assert_eq!(names(&s).len(), 7);
        assert_eq!(names(&s)[0].0, "source_id");
        let ev = SecurityEvent::default();
        let b = security_event_batch(&[(0, &ev), (1, &ev)]).unwrap();
        assert_eq!(names(&b).len(), 13);
        assert_eq!(names(&b)[12], ("row_seq".into(), false));
    }

    #[test]
    fn raw_has_utc_timestamp_and_roundtrips_parquet() {
        let r = RawRow {
            source_id: "broker:x".into(),
            message_id: "m-1".into(),
            message_type: "CSV_POSITIONS".into(),
            adapter: "tabular".into(),
            file_sha256: "0".repeat(64),
            message_offset: 0,
            message_sha256: "0".repeat(64),
            io: None,
            sender_bic: None,
            receiver_bic: None,
            sender_ref: None,
            field_count: 0,
            diagnostic_count: 0,
            diagnostics: None,
            received_at_micros: 1,
            file_name: None,
            raw_text: "x".into(),
            artifact_ref: None,
        };
        let b = raw_batch(&[r]).unwrap();
        assert_eq!(b.num_columns(), 18);
        assert!(matches!(
            b.schema().field(14).data_type(),
            DataType::Timestamp(TimeUnit::Microsecond, Some(tz)) if &**tz == "UTC"
        ));
        assert!(!to_parquet(&b).unwrap().is_empty());
    }
}
