//! Tier-1 P&L / reconciliation over normalized ingestion events (ADR-0001) —
//! the point where the wedge closes: `raw SWIFT/MX/CSV → normalized events →
//! P&L`. Tier-1 is **cash- and position-based** and needs *no market data*:
//! cash income/charges + net movement + statement reconciliation, and a
//! positions snapshot. Full mark-to-market P&L + risk is Tier-2 (the quant
//! engine consuming the Parquet seam).
//!
//! Pure and source-agnostic: it consumes [`ingest_core`] events, so SWIFT, MX
//! camt.053, and custodian CSV all feed the same P&L.

use std::collections::BTreeMap;

use ingest_core::{CashStatement, SecurityEvent};
use serde::Serialize;

/// Tier-1 cash P&L: income (inflows), charges (outflows), net, a per-type
/// breakdown, and whether every balance-bearing statement reconciles.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Tier1CashPnl {
    /// Σ positive signed amounts (coupons, dividends, interest, receipts).
    pub inflows: f64,
    /// Σ negative signed amounts (fees, commissions, payments) — negative.
    pub outflows: f64,
    /// `inflows + outflows`.
    pub net_movement: f64,
    /// Signed sum per bank transaction type (TRF, DIV, INT, CHG, …).
    pub by_type: BTreeMap<String, f64>,
    pub statements: usize,
    pub entries: usize,
    /// True when at least one statement carried balances and every such
    /// statement reconciles (opening + Σ movements == closing).
    pub all_reconciled: bool,
}

/// Compute Tier-1 cash P&L across a set of normalized cash statements.
pub fn compute_cash_pnl(statements: &[CashStatement]) -> Tier1CashPnl {
    let mut r = Tier1CashPnl {
        statements: statements.len(),
        ..Default::default()
    };
    let mut any_balances = false;
    let mut all_ok = true;
    for s in statements {
        for e in &s.entries {
            r.entries += 1;
            if e.signed_amount >= 0.0 {
                r.inflows += e.signed_amount;
            } else {
                r.outflows += e.signed_amount;
            }
            if let Some(t) = &e.transaction_type {
                *r.by_type.entry(t.clone()).or_insert(0.0) += e.signed_amount;
            }
        }
        if let Some(ok) = s.reconciles() {
            any_balances = true;
            all_ok &= ok;
        }
    }
    r.net_movement = r.inflows + r.outflows;
    r.all_reconciled = any_balances && all_ok;
    r
}

/// A positions snapshot from securities events (Tier-1: quantities per ISIN,
/// no market value).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PositionSummary {
    pub positions: usize,
    /// Total quantity per ISIN.
    pub by_isin: BTreeMap<String, f64>,
    /// Events with no resolvable ISIN (a data-quality signal).
    pub missing_isin: usize,
}

/// Summarize normalized securities events into a positions snapshot.
pub fn summarize_positions(events: &[SecurityEvent]) -> PositionSummary {
    let mut p = PositionSummary {
        positions: events.len(),
        ..Default::default()
    };
    for e in events {
        match (&e.isin, e.quantity) {
            (Some(isin), Some(q)) => *p.by_isin.entry(isin.clone()).or_insert(0.0) += q,
            (None, _) => p.missing_isin += 1,
            _ => {}
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use ingest_core::{Balance, CashEntry, Direction, EventKind};

    fn entry(dir: Direction, amount: f64, ttype: &str) -> CashEntry {
        CashEntry {
            direction: dir,
            amount,
            signed_amount: dir.sign() * amount,
            transaction_type: Some(ttype.into()),
            ..Default::default()
        }
    }

    #[test]
    fn cash_pnl_splits_income_and_charges() {
        let stmt = CashStatement {
            opening_balance: Some(Balance {
                amount: 100000.0,
                ..Default::default()
            }),
            closing_balance: Some(Balance {
                amount: 120000.0,
                ..Default::default()
            }),
            entries: vec![
                entry(Direction::Credit, 25000.0, "DIV"),
                entry(Direction::Debit, 5000.0, "CHG"),
            ],
            ..Default::default()
        };
        let p = compute_cash_pnl(std::slice::from_ref(&stmt));
        assert_eq!(p.inflows, 25000.0);
        assert_eq!(p.outflows, -5000.0);
        assert_eq!(p.net_movement, 20000.0);
        assert_eq!(p.by_type.get("DIV"), Some(&25000.0));
        assert_eq!(p.by_type.get("CHG"), Some(&-5000.0));
        assert!(p.all_reconciled);
    }

    #[test]
    fn positions_snapshot_aggregates_by_isin() {
        let events = vec![
            SecurityEvent {
                kind: EventKind::Holding,
                isin: Some("GB00B03MLX29".into()),
                quantity: Some(1000.0),
                ..Default::default()
            },
            SecurityEvent {
                kind: EventKind::Holding,
                isin: Some("GB00B03MLX29".into()),
                quantity: Some(500.0),
                ..Default::default()
            },
            SecurityEvent {
                kind: EventKind::Holding,
                isin: None,
                quantity: Some(10.0),
                ..Default::default()
            },
        ];
        let p = summarize_positions(&events);
        assert_eq!(p.positions, 3);
        assert_eq!(p.by_isin.get("GB00B03MLX29"), Some(&1500.0));
        assert_eq!(p.missing_isin, 1);
    }

    // The wedge, end to end: raw MT940 .fin -> normalized CashStatement -> Tier-1 P&L.
    #[test]
    fn mt940_fixture_closes_the_wedge() {
        let fin = include_str!("../../../examples/mt940_sample.fin");
        let stmt = swift_mt940::parse_mt940(fin, "m").expect("parse mt940");
        let p = compute_cash_pnl(std::slice::from_ref(&stmt));
        assert_eq!(p.inflows, 25000.0);
        assert_eq!(p.outflows, -5000.0);
        assert_eq!(p.net_movement, 20000.0);
        assert_eq!(p.by_type.get("TRF"), Some(&25000.0));
        assert_eq!(p.by_type.get("CHG"), Some(&-5000.0));
        assert!(p.all_reconciled);
    }
}
