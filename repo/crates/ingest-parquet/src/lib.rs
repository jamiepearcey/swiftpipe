//! The **data-plane seam**: emit normalized ingestion events as columnar
//! **Parquet** (ADR-0001). Any source adapter's output — securities
//! [`SecurityEvent`]s or cash [`CashStatement`]s — becomes a Parquet file the
//! quant engine / ArrowRef / DuckDB can read directly.
//!
//! Parquet is the interop boundary: the producer's Arrow version is irrelevant
//! to the consumer, so this crate can pin any Arrow while the engine reads the
//! files with its own.

use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use arrow::array::{ArrayRef, Float64Array, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::error::ArrowError;
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;

use ingest_core::{CashStatement, SecurityEvent};

/// Build an Arrow RecordBatch for a set of securities events.
pub fn security_events_batch(events: &[SecurityEvent]) -> Result<RecordBatch, ArrowError> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("source", DataType::Utf8, false),
        Field::new("message_id", DataType::Utf8, false),
        Field::new("message_type", DataType::Utf8, false),
        Field::new("kind", DataType::Utf8, false),
        Field::new("isin", DataType::Utf8, true),
        Field::new("instrument_desc", DataType::Utf8, true),
        Field::new("quantity", DataType::Float64, true),
        Field::new("settlement_date", DataType::Utf8, true),
        Field::new("safekeeping_account", DataType::Utf8, true),
        Field::new("party_bic", DataType::Utf8, true),
    ]));
    let cols: Vec<ArrayRef> = vec![
        Arc::new(StringArray::from_iter_values(events.iter().map(|e| e.source.as_str()))),
        Arc::new(StringArray::from_iter_values(events.iter().map(|e| e.message_id.as_str()))),
        Arc::new(StringArray::from_iter_values(events.iter().map(|e| e.message_type.as_str()))),
        Arc::new(StringArray::from_iter_values(events.iter().map(|e| e.kind.as_str()))),
        Arc::new(StringArray::from_iter(events.iter().map(|e| e.isin.clone()))),
        Arc::new(StringArray::from_iter(events.iter().map(|e| e.instrument_desc.clone()))),
        Arc::new(Float64Array::from_iter(events.iter().map(|e| e.quantity))),
        Arc::new(StringArray::from_iter(events.iter().map(|e| e.settlement_date.clone()))),
        Arc::new(StringArray::from_iter(events.iter().map(|e| e.safekeeping_account.clone()))),
        Arc::new(StringArray::from_iter(events.iter().map(|e| e.party_bic.clone()))),
    ];
    RecordBatch::try_new(schema, cols)
}

/// Build an Arrow RecordBatch of cash statement **headers** — one row per
/// statement, carrying the balances the recon identity needs (opening + Σ
/// movements == closing). Signed to the balance's effect (credit +, debit −);
/// null when a balance is absent. Pair with [`cash_entries_batch`] (the lines)
/// joined on `(source, message_id)`: header + fact is the read-model star.
pub fn cash_statements_batch(statements: &[CashStatement]) -> Result<RecordBatch, ArrowError> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("source", DataType::Utf8, false),
        Field::new("message_id", DataType::Utf8, false),
        Field::new("message_type", DataType::Utf8, false),
        Field::new("account", DataType::Utf8, true),
        Field::new("currency", DataType::Utf8, true),
        Field::new("opening", DataType::Float64, true),
        Field::new("closing", DataType::Float64, true),
    ]));
    let cols: Vec<ArrayRef> = vec![
        Arc::new(StringArray::from_iter_values(statements.iter().map(|s| s.source.as_str()))),
        Arc::new(StringArray::from_iter_values(statements.iter().map(|s| s.message_id.as_str()))),
        Arc::new(StringArray::from_iter_values(statements.iter().map(|s| s.message_type.as_str()))),
        Arc::new(StringArray::from_iter(statements.iter().map(|s| s.account.clone()))),
        Arc::new(StringArray::from_iter(statements.iter().map(|s| s.currency.clone()))),
        Arc::new(Float64Array::from_iter(statements.iter().map(|s| s.opening_balance.as_ref().map(|b| b.signed())))),
        Arc::new(Float64Array::from_iter(statements.iter().map(|s| s.closing_balance.as_ref().map(|b| b.signed())))),
    ];
    RecordBatch::try_new(schema, cols)
}

/// Build an Arrow RecordBatch of cash entries — one row per entry, carrying its
/// statement context (account/currency). This is the P&L/recon-ready shape:
/// sum `signed_amount` for net movement, group by `transaction_type` for income.
pub fn cash_entries_batch(statements: &[CashStatement]) -> Result<RecordBatch, ArrowError> {
    let rows: Vec<(&CashStatement, &ingest_core::CashEntry)> = statements
        .iter()
        .flat_map(|s| s.entries.iter().map(move |e| (s, e)))
        .collect();

    let schema = Arc::new(Schema::new(vec![
        Field::new("source", DataType::Utf8, false),
        Field::new("message_id", DataType::Utf8, false),
        Field::new("account", DataType::Utf8, true),
        Field::new("currency", DataType::Utf8, true),
        Field::new("value_date", DataType::Utf8, true),
        Field::new("entry_date", DataType::Utf8, true),
        Field::new("direction", DataType::Utf8, false),
        Field::new("amount", DataType::Float64, false),
        Field::new("signed_amount", DataType::Float64, false),
        Field::new("transaction_type", DataType::Utf8, true),
        Field::new("customer_ref", DataType::Utf8, true),
        Field::new("bank_ref", DataType::Utf8, true),
        Field::new("info", DataType::Utf8, true),
    ]));
    let cols: Vec<ArrayRef> = vec![
        Arc::new(StringArray::from_iter_values(rows.iter().map(|(s, _)| s.source.as_str()))),
        Arc::new(StringArray::from_iter_values(rows.iter().map(|(s, _)| s.message_id.as_str()))),
        Arc::new(StringArray::from_iter(rows.iter().map(|(s, _)| s.account.clone()))),
        Arc::new(StringArray::from_iter(rows.iter().map(|(s, _)| s.currency.clone()))),
        Arc::new(StringArray::from_iter(rows.iter().map(|(_, e)| e.value_date.clone()))),
        Arc::new(StringArray::from_iter(rows.iter().map(|(_, e)| e.entry_date.clone()))),
        Arc::new(StringArray::from_iter_values(rows.iter().map(|(_, e)| e.direction.as_str()))),
        Arc::new(Float64Array::from_iter_values(rows.iter().map(|(_, e)| e.amount))),
        Arc::new(Float64Array::from_iter_values(rows.iter().map(|(_, e)| e.signed_amount))),
        Arc::new(StringArray::from_iter(rows.iter().map(|(_, e)| e.transaction_type.clone()))),
        Arc::new(StringArray::from_iter(rows.iter().map(|(_, e)| e.customer_ref.clone()))),
        Arc::new(StringArray::from_iter(rows.iter().map(|(_, e)| e.bank_ref.clone()))),
        Arc::new(StringArray::from_iter(rows.iter().map(|(_, e)| e.info.clone()))),
    ];
    RecordBatch::try_new(schema, cols)
}

/// Write a RecordBatch to a Parquet file.
pub fn write_parquet(batch: &RecordBatch, path: &Path) -> anyhow::Result<()> {
    let file = File::create(path)?;
    let mut writer = ArrowWriter::try_new(file, batch.schema(), None)?;
    writer.write(batch)?;
    writer.close()?;
    Ok(())
}

/// Convenience: securities events → Parquet file.
pub fn write_security_events(events: &[SecurityEvent], path: &Path) -> anyhow::Result<()> {
    write_parquet(&security_events_batch(events)?, path)
}

/// Convenience: cash statements → Parquet file (one row per entry).
pub fn write_cash_entries(statements: &[CashStatement], path: &Path) -> anyhow::Result<()> {
    write_parquet(&cash_entries_batch(statements)?, path)
}

/// Convenience: cash statement headers → Parquet file (one row per statement).
pub fn write_cash_statements(statements: &[CashStatement], path: &Path) -> anyhow::Result<()> {
    write_parquet(&cash_statements_batch(statements)?, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ingest_core::{Balance, CashEntry, Direction, EventKind};
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

    fn read_rows(path: &Path) -> Vec<RecordBatch> {
        let file = File::open(path).unwrap();
        ParquetRecordBatchReaderBuilder::try_new(file)
            .unwrap()
            .build()
            .unwrap()
            .map(|b| b.unwrap())
            .collect()
    }

    #[test]
    fn security_events_round_trip_parquet() {
        let events = vec![
            SecurityEvent {
                source: "swift".into(),
                message_id: "m1".into(),
                message_type: "MT535".into(),
                kind: EventKind::Holding,
                isin: Some("GB00B03MLX29".into()),
                quantity: Some(1000.0),
                ..Default::default()
            },
            SecurityEvent {
                source: "tabular".into(),
                message_id: "m2".into(),
                message_type: "CSV_POSITIONS".into(),
                kind: EventKind::Holding,
                isin: Some("US0378331005".into()),
                quantity: Some(250.5),
                ..Default::default()
            },
        ];
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("positions.parquet");
        write_security_events(&events, &path).unwrap();

        let batches = read_rows(&path);
        let rows: usize = batches.iter().map(|b| b.num_rows()).sum();
        assert_eq!(rows, 2);
        assert_eq!(batches[0].num_columns(), 10);
    }

    #[test]
    fn cash_entries_round_trip_parquet() {
        let stmt = CashStatement {
            source: "swift".into(),
            message_id: "s1".into(),
            message_type: "MT940".into(),
            account: Some("GB29...".into()),
            currency: Some("EUR".into()),
            opening_balance: Some(Balance { amount: 100000.0, ..Default::default() }),
            closing_balance: Some(Balance { amount: 120000.0, ..Default::default() }),
            entries: vec![
                CashEntry { direction: Direction::Credit, amount: 25000.0, signed_amount: 25000.0, transaction_type: Some("TRF".into()), ..Default::default() },
                CashEntry { direction: Direction::Debit, amount: 5000.0, signed_amount: -5000.0, transaction_type: Some("CHG".into()), ..Default::default() },
            ],
            ..Default::default()
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cash.parquet");
        write_cash_entries(std::slice::from_ref(&stmt), &path).unwrap();

        let batches = read_rows(&path);
        let rows: usize = batches.iter().map(|b| b.num_rows()).sum();
        assert_eq!(rows, 2, "one row per cash entry");
        assert_eq!(batches[0].num_columns(), 13);
    }

    #[test]
    fn cash_statement_headers_persist_balances() {
        let stmt = CashStatement {
            source: "swift".into(),
            message_id: "s1".into(),
            message_type: "MT940".into(),
            account: Some("GB29...".into()),
            currency: Some("EUR".into()),
            opening_balance: Some(Balance { direction: Direction::Credit, amount: 100000.0, ..Default::default() }),
            closing_balance: Some(Balance { direction: Direction::Credit, amount: 120000.0, ..Default::default() }),
            ..Default::default()
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cash_statements.parquet");
        write_cash_statements(std::slice::from_ref(&stmt), &path).unwrap();

        let batches = read_rows(&path);
        let rows: usize = batches.iter().map(|b| b.num_rows()).sum();
        assert_eq!(rows, 1, "one row per statement header");
        assert_eq!(batches[0].num_columns(), 7);
    }
}
