//! The **recon snapshot** — a stable, UI-facing JSON view over the normalized
//! ingestion model. It is the presentation-plane twin of the Parquet seam: the
//! same normalized `CashStatement` / `SecurityEvent` data, flattened to exactly
//! the contract the workbench's Reconciliation desk consumes (camelCase, single
//! opening/closing numbers). JSON is the interop boundary here — the UI never
//! sees swiftpipe types, only this snapshot, so the two sides evolve
//! independently (same discipline as Arrow/Parquet on the data plane).

use ingest_core::{CashStatement, SecurityEvent};
use serde::Serialize;

/// One cash movement, flattened for the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotEntry {
    pub value_date: String,
    pub direction: String,
    pub amount: f64,
    /// Effect on the balance (credit +, debit −).
    pub signed_amount: f64,
    pub transaction_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub info: Option<String>,
}

/// One cash statement, flattened for the UI (opening/closing as signed numbers).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotStatement {
    pub source: String,
    pub message_id: String,
    pub message_type: String,
    pub account: String,
    pub currency: String,
    pub opening: f64,
    pub closing: f64,
    pub entries: Vec<SnapshotEntry>,
}

/// One position/holding, flattened for the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotPosition {
    pub source: String,
    pub message_type: String,
    pub isin: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desc: Option<String>,
    pub quantity: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safekeeping_account: Option<String>,
}

/// The whole snapshot the UI fetches (`recon-snapshot.json`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReconSnapshot {
    /// Schema tag so the UI can reject/adapt older shapes.
    pub version: u32,
    pub statements: Vec<SnapshotStatement>,
    pub positions: Vec<SnapshotPosition>,
}

impl ReconSnapshot {
    pub const VERSION: u32 = 1;

    pub fn build(statements: &[CashStatement], events: &[SecurityEvent]) -> Self {
        ReconSnapshot {
            version: Self::VERSION,
            statements: statements.iter().map(map_statement).collect(),
            positions: events.iter().filter_map(map_position).collect(),
        }
    }

    /// Assemble from already-flattened parts — the read-model service builds
    /// these straight from a DuckDB query over the Parquet store.
    pub fn from_parts(
        statements: Vec<SnapshotStatement>,
        positions: Vec<SnapshotPosition>,
    ) -> Self {
        ReconSnapshot {
            version: Self::VERSION,
            statements,
            positions,
        }
    }
}

fn map_statement(s: &CashStatement) -> SnapshotStatement {
    SnapshotStatement {
        source: s.source.clone(),
        message_id: s.message_id.clone(),
        message_type: s.message_type.clone(),
        account: s.account.clone().unwrap_or_default(),
        currency: s.currency.clone().unwrap_or_default(),
        opening: s
            .opening_balance
            .as_ref()
            .map(|b| b.signed())
            .unwrap_or(0.0),
        closing: s
            .closing_balance
            .as_ref()
            .map(|b| b.signed())
            .unwrap_or(0.0),
        entries: s.entries.iter().map(map_entry).collect(),
    }
}

fn map_entry(e: &ingest_core::CashEntry) -> SnapshotEntry {
    SnapshotEntry {
        value_date: e
            .value_date
            .clone()
            .or_else(|| e.entry_date.clone())
            .unwrap_or_default(),
        direction: e.direction.as_str().to_string(),
        amount: e.amount,
        signed_amount: e.signed_amount,
        transaction_type: e.transaction_type.clone().unwrap_or_default(),
        reference: e.customer_ref.clone().or_else(|| e.bank_ref.clone()),
        info: e.info.clone(),
    }
}

/// Positions come from holding/settlement events that carry an ISIN + quantity.
fn map_position(ev: &SecurityEvent) -> Option<SnapshotPosition> {
    let isin = ev.isin.clone()?;
    Some(SnapshotPosition {
        source: ev.source.clone(),
        message_type: ev.message_type.clone(),
        isin,
        desc: ev.instrument_desc.clone(),
        quantity: ev.quantity.unwrap_or(0.0),
        safekeeping_account: ev.safekeeping_account.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ingest_core::{Balance, CashEntry, Direction, EventKind};

    fn bal(dir: Direction, amount: f64) -> Balance {
        Balance {
            direction: dir,
            date: None,
            currency: Some("EUR".into()),
            amount,
        }
    }

    #[test]
    fn flattens_statement_to_ui_contract() {
        let stmt = CashStatement {
            source: "swift".into(),
            message_id: "STMT1".into(),
            message_type: "MT940".into(),
            account: Some("GB29BANK".into()),
            currency: Some("EUR".into()),
            opening_balance: Some(bal(Direction::Credit, 100_000.0)),
            closing_balance: Some(bal(Direction::Credit, 120_000.0)),
            available_balance: None,
            entries: vec![CashEntry {
                value_date: Some("2026-05-13".into()),
                direction: Direction::Credit,
                amount: 20_000.0,
                signed_amount: 20_000.0,
                transaction_type: Some("TRF".into()),
                info: Some("Coupon".into()),
                customer_ref: Some("REF1".into()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let snap = ReconSnapshot::build(&[stmt], &[]);
        assert_eq!(snap.version, 1);
        let s = &snap.statements[0];
        assert_eq!(s.opening, 100_000.0);
        assert_eq!(s.closing, 120_000.0);
        assert_eq!(s.entries[0].reference.as_deref(), Some("REF1"));
        // Round-trips to the camelCase the UI expects.
        let json = serde_json::to_string(&snap).unwrap();
        assert!(json.contains("\"messageId\""));
        assert!(json.contains("\"signedAmount\""));
    }

    #[test]
    fn positions_need_an_isin() {
        let with = SecurityEvent {
            source: "swift".into(),
            message_type: "MT535".into(),
            kind: EventKind::Holding,
            isin: Some("GB00B03MLX29".into()),
            quantity: Some(1000.0),
            ..Default::default()
        };
        let without = SecurityEvent {
            source: "swift".into(),
            ..Default::default()
        };
        let snap = ReconSnapshot::build(&[], &[with, without]);
        assert_eq!(snap.positions.len(), 1);
        assert_eq!(snap.positions[0].isin, "GB00B03MLX29");
    }
}
