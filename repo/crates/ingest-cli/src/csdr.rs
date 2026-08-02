//! The **CSDR penalty snapshot** — the UI-facing JSON view over the penalty
//! book, the presentation-plane twin of `snapshot.rs`. It flattens the pure
//! `ingest-penalty` accrual + reconciliation result to exactly the contract the
//! CSDR recon desks (swiftpipe console + quant/pricing) consume (camelCase),
//! and — like the recon snapshot — is the interop boundary so the UI never sees
//! swiftpipe types, only this snapshot.

use ingest_penalty::{reconcile_penalties, PenaltyAccrual, PenaltyReconSummary, ReportedPenalty};
use serde::Serialize;

/// One computed expected penalty, flattened for the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CsdrAccrual {
    pub source: String,
    pub transaction_ref: String,
    pub isin: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instrument_desc: Option<String>,
    pub instrument_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counterparty: Option<String>,
    pub currency: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantity: Option<f64>,
    pub reference_amount: f64,
    pub penalty_type: String,
    pub penalty_rate_bps: f64,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intended_settlement_date: Option<String>,
    pub computed_amount: f64,
    pub direction: String,
}

/// One reconciled line (computed vs CSD-reported), flattened for the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CsdrReconLine {
    pub transaction_ref: String,
    pub isin: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counterparty: Option<String>,
    pub currency: String,
    pub penalty_type: String,
    pub computed: f64,
    pub reported: f64,
    pub diff: f64,
    /// `matched` | `break` | `missing_reported` | `missing_computed`.
    pub status: String,
}

/// Aggregate recon summary, flattened for the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CsdrSummary {
    pub computed_total: f64,
    pub reported_total: f64,
    pub net_diff: f64,
    pub accruals: usize,
    pub reported: usize,
    pub matched: usize,
    pub breaks: usize,
    pub missing_reported: usize,
    pub missing_computed: usize,
    pub break_amount: f64,
    pub all_reconciled: bool,
}

impl From<PenaltyReconSummary> for CsdrSummary {
    fn from(s: PenaltyReconSummary) -> Self {
        CsdrSummary {
            computed_total: s.computed_total,
            reported_total: s.reported_total,
            net_diff: s.net_diff,
            accruals: s.accruals,
            reported: s.reported,
            matched: s.matched,
            breaks: s.breaks,
            missing_reported: s.missing_reported,
            missing_computed: s.missing_computed,
            break_amount: s.break_amount,
            all_reconciled: s.all_reconciled,
        }
    }
}

/// The whole CSDR snapshot the UI fetches (`GET /csdr/snapshot`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CsdrSnapshot {
    /// Schema tag so the UI can reject/adapt older shapes.
    pub version: u32,
    pub summary: CsdrSummary,
    pub accruals: Vec<CsdrAccrual>,
    pub lines: Vec<CsdrReconLine>,
}

impl CsdrSnapshot {
    pub const VERSION: u32 = 1;

    /// Build from computed accruals + CSD-reported penalties: runs the pure
    /// reconciliation and flattens both sides to the UI contract.
    pub fn from_penalties(accruals: Vec<PenaltyAccrual>, reported: Vec<ReportedPenalty>) -> Self {
        let (lines, summary) = reconcile_penalties(&accruals, &reported);
        CsdrSnapshot {
            version: Self::VERSION,
            summary: summary.into(),
            accruals: accruals.into_iter().map(map_accrual).collect(),
            lines: lines
                .into_iter()
                .map(|l| CsdrReconLine {
                    transaction_ref: l.transaction_ref,
                    isin: l.isin,
                    counterparty: l.counterparty_bic,
                    currency: l.currency,
                    penalty_type: l.penalty_type,
                    computed: l.computed_amount,
                    reported: l.reported_amount,
                    diff: l.diff,
                    status: l.status,
                })
                .collect(),
        }
    }
}

fn map_accrual(a: PenaltyAccrual) -> CsdrAccrual {
    CsdrAccrual {
        source: a.source,
        transaction_ref: a.transaction_ref,
        isin: a.isin,
        instrument_desc: a.instrument_desc,
        instrument_type: a.instrument_type,
        counterparty: a.counterparty_bic,
        currency: a.currency,
        quantity: a.quantity,
        reference_amount: a.reference_amount,
        penalty_type: a.penalty_type,
        penalty_rate_bps: a.penalty_rate_bps,
        status: a.status,
        intended_settlement_date: a.intended_settlement_date,
        computed_amount: a.computed_amount,
        direction: a.direction,
    }
}
