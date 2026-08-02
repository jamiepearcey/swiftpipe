//! CSDR cash-penalty engine (ADR-0012) — pure and source-agnostic, mirroring
//! [`ingest_pnl`]. It computes **expected** CSDR Settlement Discipline cash
//! penalties from normalized failing/pending securities events (MT537 *Statement
//! of Pending Transactions*), and reconciles them against the CSD's monthly
//! **reported** penalty statement (computed vs reported → breaks) — the same
//! recon shape as the MT940 cash reconciliation, but for the penalty book.
//!
//! Penalty (SEFP, Settlement Fail Penalty) = `rate_bps/10_000 × reference_amount
//! × business_days_failed`, where the rate is set by instrument type per the
//! Annex of Commission Delegated Regulation (EU) 2017/389.
//!
//! Amounts are **signed by direction**: `payable` (the account owner owes the
//! CSD) is negative, `receivable` (the CSD owes the account owner) is
//! positive — see [`PenaltyAccrual::computed_amount`] for the full rationale.
//!
//! **Starter, not certified** (see ADR-0012): the rate table is a configurable
//! representative default, `business_days_failed` defaults to 1, only SEFP is
//! computed (LMFP deferred), currency is best-effort, and instrument
//! classification is a heuristic. Consumes only [`ingest_core`] types.

use std::collections::BTreeMap;

use ingest_core::SecurityEvent;
use serde::Serialize;

// ---------------------------------------------------------------------------
// Instrument classification + rate table
// ---------------------------------------------------------------------------

/// Instrument category that determines the penalty rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstrumentType {
    LiquidShare,
    IlliquidShare,
    SmeGrowthShare,
    CorporateBond,
    SmeGrowthBond,
    SovereignBond,
    Etf,
    Other,
}

impl InstrumentType {
    pub fn as_str(self) -> &'static str {
        match self {
            InstrumentType::LiquidShare => "liquid_share",
            InstrumentType::IlliquidShare => "illiquid_share",
            InstrumentType::SmeGrowthShare => "sme_growth_share",
            InstrumentType::CorporateBond => "corporate_bond",
            InstrumentType::SmeGrowthBond => "sme_growth_bond",
            InstrumentType::SovereignBond => "sovereign_bond",
            InstrumentType::Etf => "etf",
            InstrumentType::Other => "other",
        }
    }
}

/// Penalty rates in basis points per instrument type. **Starter defaults** from
/// the Annex of CDR (EU) 2017/389 — representative, not certified. Real use
/// needs the certified rates + a MiFID II liquidity classification per ISIN.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct PenaltyRateTable {
    pub liquid_share: f64,
    pub illiquid_share: f64,
    pub sme_growth_share: f64,
    pub corporate_bond: f64,
    pub sme_growth_bond: f64,
    pub sovereign_bond: f64,
    pub etf: f64,
    pub other: f64,
}

impl Default for PenaltyRateTable {
    fn default() -> Self {
        Self::starter()
    }
}

impl PenaltyRateTable {
    /// Representative starter rate table (bps).
    pub fn starter() -> Self {
        PenaltyRateTable {
            liquid_share: 1.0,
            illiquid_share: 0.5,
            sme_growth_share: 0.25,
            corporate_bond: 0.20,
            sme_growth_bond: 0.15,
            sovereign_bond: 0.10,
            etf: 0.5,
            other: 0.5,
        }
    }

    pub fn rate_bps(&self, t: InstrumentType) -> f64 {
        match t {
            InstrumentType::LiquidShare => self.liquid_share,
            InstrumentType::IlliquidShare => self.illiquid_share,
            InstrumentType::SmeGrowthShare => self.sme_growth_share,
            InstrumentType::CorporateBond => self.corporate_bond,
            InstrumentType::SmeGrowthBond => self.sme_growth_bond,
            InstrumentType::SovereignBond => self.sovereign_bond,
            InstrumentType::Etf => self.etf,
            InstrumentType::Other => self.other,
        }
    }
}

/// Heuristic instrument classification from the instrument description.
/// Starter only — a real system keys off reference data + MiFID II liquidity
/// classification per ISIN.
///
/// Deliberately takes no ISIN: the ISO 6166 ISIN prefix identifies the
/// *country of the issuing CSD/registrar*, not the instrument category —
/// `GB`-prefixed ISINs are overwhelmingly ordinary UK equities and corporate
/// bonds, not gilts, so treating the prefix as a sovereign signal would
/// overclaim precision the starter doesn't have. Real classification needs
/// reference data (issuer type), not the ISIN string.
pub fn classify_instrument(desc: Option<&str>) -> InstrumentType {
    let d = desc.unwrap_or("").to_ascii_uppercase();
    let sovereign = ["GILT", "TREASURY", "TREAS", "BUND", "BTP", "OAT", "GOVT", "SOVEREIGN", "T-BILL"];
    if sovereign.iter().any(|k| d.contains(k)) {
        return InstrumentType::SovereignBond;
    }
    if d.contains("ETF") {
        return InstrumentType::Etf;
    }
    if d.contains("BOND") || d.contains("NOTE") || d.contains("FRN") || d.contains('%') {
        return InstrumentType::CorporateBond;
    }
    if d.contains("SHARE") || d.contains("ORD") || d.contains("EQUITY") || d.contains("PLC") {
        return InstrumentType::LiquidShare;
    }
    // No signal in the description: default to a liquid share (the most
    // common, highest-rate case) — documented starter behavior, not a claim
    // of accuracy.
    InstrumentType::LiquidShare
}

// ---------------------------------------------------------------------------
// Penalty accrual (expected) + reported statement line
// ---------------------------------------------------------------------------

/// Statuses (ISO 15022 `:25D::IPRC//`) that indicate a transaction is failing
/// or pending and therefore accrues a settlement-fail penalty.
pub fn is_penalty_relevant(status: &str) -> bool {
    matches!(status.trim().to_ascii_uppercase().as_str(), "PEND" | "PENF")
}

/// CSDR cash-penalty type, per the Annex of CDR (EU) 2017/389. Starter scope:
/// [`compute_penalty_accruals`] only ever computes `Sefp`; `Lmfp` is deferred
/// but modeled so a CSD statement line carrying it round-trips instead of
/// being silently coerced to `Sefp`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum PenaltyType {
    /// Settlement Fail Penalty.
    #[default]
    Sefp,
    /// Late-Matching Fail Penalty (retroactive to intended settlement date).
    Lmfp,
}

impl PenaltyType {
    /// The stable wire/Parquet representation (`"SEFP"` / `"LMFP"`) — this is
    /// what [`PenaltyAccrual::penalty_type`] / [`ReportedPenalty::penalty_type`]
    /// actually store, and what the CSDR UIs key their display off.
    pub fn as_str(self) -> &'static str {
        match self {
            PenaltyType::Sefp => "SEFP",
            PenaltyType::Lmfp => "LMFP",
        }
    }

    /// Parse a CSD statement's `penalty_type` column (case-insensitive).
    /// Unrecognised values return `None` — the starter only models SEFP/LMFP.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_uppercase().as_str() {
            "SEFP" => Some(PenaltyType::Sefp),
            "LMFP" => Some(PenaltyType::Lmfp),
            _ => None,
        }
    }
}

/// An **expected** CSDR cash penalty computed from one failing/pending event.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PenaltyAccrual {
    pub source: String,
    /// Transaction reference the CSD statement keys on (MT537 `:20C::RELA`,
    /// else `:20C::SEME`) — **not** the message id: one MT537 message can
    /// carry many failing transactions, each with its own reference.
    pub transaction_ref: String,
    pub isin: String,
    pub instrument_desc: Option<String>,
    pub instrument_type: String,
    pub counterparty_bic: Option<String>,
    pub currency: String,
    pub quantity: Option<f64>,
    /// Settlement/posting amount the penalty is computed against.
    pub reference_amount: f64,
    /// Wire value `"SEFP"` / `"LMFP"` — see [`PenaltyType`].
    pub penalty_type: String,
    pub penalty_rate_bps: f64,
    pub status: String,
    pub intended_settlement_date: Option<String>,
    pub business_days_failed: u32,
    /// `rate_bps/10_000 × reference_amount × business_days_failed`, **signed
    /// by [`direction`](Self::direction)**: negative when `payable` (the
    /// account owner owes the CSD), positive when `receivable` (the CSD owes
    /// the account owner). Summing signed amounts gives the net penalty
    /// position; it also means a computed payable can never accidentally
    /// cancel out a reported receivable of the same magnitude during
    /// reconciliation — the opposite signs surface the full swing as a break
    /// instead of netting to zero.
    pub computed_amount: f64,
    /// Who owes the penalty from the account owner's view: `"payable"` (owed
    /// to the CSD, negative `computed_amount`) or `"receivable"` (owed by the
    /// CSD, positive `computed_amount`). Starter: accruals computed from the
    /// account owner's own failing/pending events are always `"payable"`.
    pub direction: String,
}

/// A line from the CSD's monthly **reported** penalty statement.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ReportedPenalty {
    pub penalty_ref: String,
    pub transaction_ref: String,
    pub isin: String,
    pub counterparty_bic: Option<String>,
    pub currency: String,
    /// Wire value `"SEFP"` / `"LMFP"` — see [`PenaltyType`].
    pub penalty_type: String,
    pub period: Option<String>,
    /// The statement's reported amount, **signed by [`direction`](Self::direction)**
    /// using the same convention as [`PenaltyAccrual::computed_amount`]:
    /// negative when `payable`, positive when `receivable`.
    pub reported_amount: f64,
    /// `"payable"` (negative `reported_amount`) or `"receivable"` (positive
    /// `reported_amount`) — see [`PenaltyAccrual::direction`].
    pub direction: String,
}

/// Apply the CSDR direction sign convention to an unsigned magnitude:
/// `payable` → negative (cash the account owner owes out), anything else
/// (`receivable`) → positive (cash owed to the account owner). Unrecognised
/// or missing directions default to `payable`, matching the field's own
/// default.
fn signed_amount(magnitude: f64, direction: &str) -> f64 {
    if direction.trim().eq_ignore_ascii_case("receivable") {
        magnitude.abs()
    } else {
        -magnitude.abs()
    }
}

/// Compute expected SEFP accruals from normalized securities events. Only events
/// with a penalty-relevant status, an ISIN, and a reference amount accrue.
pub fn compute_penalty_accruals(
    events: &[SecurityEvent],
    rates: &PenaltyRateTable,
    default_currency: &str,
) -> Vec<PenaltyAccrual> {
    events
        .iter()
        .filter_map(|e| {
            let status = e.status.as_deref()?;
            if !is_penalty_relevant(status) {
                return None;
            }
            let isin = e.isin.clone()?;
            let amount = e.amount?;
            let itype = classify_instrument(e.instrument_desc.as_deref());
            let rate = rates.rate_bps(itype);
            let days = 1u32; // starter: single-day accrual
            let computed = rate / 10_000.0 * amount * days as f64;
            let direction = "payable".to_string();
            Some(PenaltyAccrual {
                source: e.source.clone(),
                // The per-transaction reference, not the message id — a
                // single MT537 message can carry many failing transactions.
                transaction_ref: e.transaction_ref.clone().unwrap_or_else(|| e.message_id.clone()),
                isin,
                instrument_desc: e.instrument_desc.clone(),
                instrument_type: itype.as_str().to_string(),
                counterparty_bic: e.party_bic.clone(),
                currency: e.currency.clone().unwrap_or_else(|| default_currency.to_string()),
                quantity: e.quantity,
                reference_amount: amount,
                penalty_type: PenaltyType::Sefp.as_str().to_string(),
                penalty_rate_bps: rate,
                status: status.to_string(),
                intended_settlement_date: e.settlement_date.clone(),
                business_days_failed: days,
                computed_amount: round2(signed_amount(computed, &direction)),
                direction,
            })
        })
        .collect()
}

/// Parse a CSD monthly penalty statement CSV. Header-driven and lenient;
/// missing optional columns are tolerated, and quoted fields (RFC 4180) are
/// handled correctly — instrument descriptions and counterparty names commonly
/// contain commas. Recognised columns (case-insensitive): `penalty_ref,
/// transaction_ref, isin, counterparty, currency, penalty_type, amount,
/// period, direction`. `amount` is read as an unsigned magnitude and signed by
/// `direction` (default `payable`) into [`ReportedPenalty::reported_amount`].
pub fn parse_penalty_statement_csv(csv_text: &str) -> Vec<ReportedPenalty> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(csv_text.as_bytes());

    let header: Vec<String> = match reader.headers() {
        Ok(h) => h.iter().map(str::to_ascii_lowercase).collect(),
        Err(_) => return Vec::new(),
    };
    let idx = |name: &str| header.iter().position(|h| h == name);
    let (i_ref, i_txn, i_isin, i_cp, i_ccy, i_type, i_amt, i_period, i_dir) = (
        idx("penalty_ref"),
        idx("transaction_ref"),
        idx("isin"),
        idx("counterparty"),
        idx("currency"),
        idx("penalty_type"),
        idx("amount"),
        idx("period"),
        idx("direction"),
    );

    reader
        .records()
        .filter_map(|record| {
            let record = record.ok()?;
            let get = |i: Option<usize>| {
                i.and_then(|i| record.get(i)).map(str::to_string).filter(|s| !s.is_empty())
            };
            let amount = get(i_amt)?.parse::<f64>().ok()?;
            let direction = get(i_dir).unwrap_or_else(|| "payable".to_string());
            let penalty_type = get(i_type).and_then(|s| PenaltyType::parse(&s)).unwrap_or_default();
            Some(ReportedPenalty {
                penalty_ref: get(i_ref).unwrap_or_default(),
                transaction_ref: get(i_txn).unwrap_or_default(),
                isin: get(i_isin).unwrap_or_default(),
                counterparty_bic: get(i_cp),
                currency: get(i_ccy).unwrap_or_default(),
                penalty_type: penalty_type.as_str().to_string(),
                period: get(i_period),
                reported_amount: round2(signed_amount(amount, &direction)),
                direction,
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Reconciliation (computed vs reported)
// ---------------------------------------------------------------------------

/// One reconciled penalty: expected (computed) vs CSD-reported.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PenaltyReconLine {
    pub key: String,
    pub transaction_ref: String,
    pub isin: String,
    pub counterparty_bic: Option<String>,
    pub currency: String,
    pub penalty_type: String,
    /// Signed by direction — see [`PenaltyAccrual::computed_amount`].
    pub computed_amount: f64,
    /// Signed by direction — see [`ReportedPenalty::reported_amount`].
    pub reported_amount: f64,
    /// `reported − computed` (both signed). A direction mismatch (e.g. a
    /// computed payable vs a reported receivable of the same magnitude) shows
    /// up as double the magnitude, not zero.
    pub diff: f64,
    /// `matched` | `break` | `missing_reported` | `missing_computed`.
    pub status: String,
}

/// Aggregate recon result over a period.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PenaltyReconSummary {
    /// Σ signed computed amounts — the net expected penalty position
    /// (negative = net payable, positive = net receivable).
    pub computed_total: f64,
    /// Σ signed reported amounts — the net CSD-reported penalty position.
    pub reported_total: f64,
    /// `reported_total − computed_total`.
    pub net_diff: f64,
    pub accruals: usize,
    pub reported: usize,
    pub matched: usize,
    pub breaks: usize,
    pub missing_reported: usize,
    pub missing_computed: usize,
    /// Σ |diff| over every non-matched line — the unsigned size of the
    /// outstanding breaks, not a net figure.
    pub break_amount: f64,
    pub all_reconciled: bool,
}

/// Absolute tolerance (currency units) within which computed == reported.
pub const RECON_TOLERANCE: f64 = 0.01;

fn recon_key(transaction_ref: &str, isin: &str, penalty_type: PenaltyType) -> String {
    format!(
        "{}|{}|{}",
        transaction_ref.trim().to_ascii_uppercase(),
        isin.trim().to_ascii_uppercase(),
        penalty_type.as_str()
    )
}

/// Reconcile expected accruals against reported penalties, keyed on
/// (transaction ref, ISIN, penalty type). Duplicate keys on either side are
/// summed (signed, so opposite-direction duplicates net correctly). Returns
/// per-key lines (sorted by key) + an aggregate summary.
pub fn reconcile_penalties(
    computed: &[PenaltyAccrual],
    reported: &[ReportedPenalty],
) -> (Vec<PenaltyReconLine>, PenaltyReconSummary) {
    struct Side {
        amount: f64,
        transaction_ref: String,
        isin: String,
        counterparty_bic: Option<String>,
        currency: String,
        penalty_type: String,
    }
    let mut sides: BTreeMap<String, (Option<Side>, Option<Side>)> = BTreeMap::new();

    for a in computed {
        let ptype = PenaltyType::parse(&a.penalty_type).unwrap_or_default();
        let key = recon_key(&a.transaction_ref, &a.isin, ptype);
        let entry = sides.entry(key).or_default();
        match &mut entry.0 {
            Some(s) => s.amount += a.computed_amount,
            None => {
                entry.0 = Some(Side {
                    amount: a.computed_amount,
                    transaction_ref: a.transaction_ref.clone(),
                    isin: a.isin.clone(),
                    counterparty_bic: a.counterparty_bic.clone(),
                    currency: a.currency.clone(),
                    penalty_type: a.penalty_type.clone(),
                })
            }
        }
    }
    for r in reported {
        let ptype = PenaltyType::parse(&r.penalty_type).unwrap_or_default();
        let key = recon_key(&r.transaction_ref, &r.isin, ptype);
        let entry = sides.entry(key).or_default();
        match &mut entry.1 {
            Some(s) => s.amount += r.reported_amount,
            None => {
                entry.1 = Some(Side {
                    amount: r.reported_amount,
                    transaction_ref: r.transaction_ref.clone(),
                    isin: r.isin.clone(),
                    counterparty_bic: r.counterparty_bic.clone(),
                    currency: r.currency.clone(),
                    penalty_type: r.penalty_type.clone(),
                })
            }
        }
    }

    let mut lines = Vec::with_capacity(sides.len());
    let mut sum = PenaltyReconSummary {
        accruals: computed.len(),
        reported: reported.len(),
        ..Default::default()
    };
    for (key, (c, r)) in sides {
        let computed_amount = c.as_ref().map(|s| s.amount).unwrap_or(0.0);
        let reported_amount = r.as_ref().map(|s| s.amount).unwrap_or(0.0);
        let repr = c.as_ref().or(r.as_ref()).expect("at least one side present");
        let diff = round2(reported_amount - computed_amount);
        let status = match (&c, &r) {
            (Some(_), Some(_)) if diff.abs() <= RECON_TOLERANCE => "matched",
            (Some(_), Some(_)) => "break",
            (Some(_), None) => "missing_reported",
            (None, Some(_)) => "missing_computed",
            (None, None) => unreachable!(),
        };
        match status {
            "matched" => sum.matched += 1,
            "break" => sum.breaks += 1,
            "missing_reported" => sum.missing_reported += 1,
            "missing_computed" => sum.missing_computed += 1,
            _ => {}
        }
        if status != "matched" {
            sum.break_amount += diff.abs();
        }
        sum.computed_total += computed_amount;
        sum.reported_total += reported_amount;
        lines.push(PenaltyReconLine {
            key,
            transaction_ref: repr.transaction_ref.clone(),
            isin: repr.isin.clone(),
            counterparty_bic: repr.counterparty_bic.clone(),
            currency: repr.currency.clone(),
            penalty_type: repr.penalty_type.clone(),
            computed_amount: round2(computed_amount),
            reported_amount: round2(reported_amount),
            diff,
            status: status.to_string(),
        });
    }
    sum.computed_total = round2(sum.computed_total);
    sum.reported_total = round2(sum.reported_total);
    sum.net_diff = round2(sum.reported_total - sum.computed_total);
    sum.break_amount = round2(sum.break_amount);
    sum.all_reconciled = sum.breaks == 0 && sum.missing_reported == 0 && sum.missing_computed == 0;
    (lines, sum)
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use ingest_core::EventKind;

    fn failing_event(reference_amount: f64, desc: &str) -> SecurityEvent {
        SecurityEvent {
            source: "swift".into(),
            message_id: "MT537REF1".into(),
            transaction_ref: Some("RELREF1".into()),
            message_type: "MT537".into(),
            kind: EventKind::Settlement,
            isin: Some("GB00B03MLX29".into()),
            instrument_desc: Some(desc.into()),
            amount: Some(reference_amount),
            party_bic: Some("BANKGB22".into()),
            settlement_date: Some("2026-05-12".into()),
            status: Some("PEND".into()),
            ..Default::default()
        }
    }

    #[test]
    fn classifies_and_rates_instruments() {
        assert_eq!(classify_instrument(Some("ACME PLC ORD")), InstrumentType::LiquidShare);
        assert_eq!(classify_instrument(Some("UK GILT 4% 2030")), InstrumentType::SovereignBond);
        assert_eq!(classify_instrument(Some("ACME 5% 2028 BOND")), InstrumentType::CorporateBond);
        let t = PenaltyRateTable::starter();
        assert_eq!(t.rate_bps(InstrumentType::LiquidShare), 1.0);
        assert_eq!(t.rate_bps(InstrumentType::SovereignBond), 0.10);
    }

    #[test]
    fn only_failing_events_with_amount_accrue() {
        let events = vec![
            failing_event(12_345.67, "ACME PLC ORD"),
            SecurityEvent { status: Some("FUTU".into()), amount: Some(1000.0), isin: Some("X".into()), ..Default::default() },
            SecurityEvent { kind: EventKind::Holding, isin: Some("Y".into()), quantity: Some(10.0), ..Default::default() },
        ];
        let acc = compute_penalty_accruals(&events, &PenaltyRateTable::starter(), "GBP");
        assert_eq!(acc.len(), 1, "only the PEND event with an amount accrues");
        let a = &acc[0];
        assert_eq!(a.transaction_ref, "RELREF1", "the per-transaction reference, not the message id");
        assert_eq!(a.instrument_type, "liquid_share");
        assert_eq!(a.penalty_rate_bps, 1.0);
        assert_eq!(a.reference_amount, 12_345.67);
        // 1.0 bps of 12,345.67 = 1.234567 → 1.23, negative: payable is owed out.
        assert_eq!(a.computed_amount, -1.23);
        assert_eq!(a.direction, "payable");
        assert_eq!(a.currency, "GBP");
        assert_eq!(a.penalty_type, "SEFP");
    }

    #[test]
    fn falls_back_to_message_id_when_no_transaction_ref() {
        let mut e = failing_event(1000.0, "ACME PLC ORD");
        e.transaction_ref = None;
        let acc = compute_penalty_accruals(&[e], &PenaltyRateTable::starter(), "GBP");
        assert_eq!(acc[0].transaction_ref, "MT537REF1");
    }

    #[test]
    fn distinct_transactions_from_one_message_do_not_collapse() {
        // Two failing transactions sharing one message_id (one MT537 message,
        // two pending transactions) must produce two distinct accrual keys —
        // this is the multi-transaction MT537 case the recon key must not merge.
        let mut a = failing_event(1000.0, "GILT 4% 2030");
        a.transaction_ref = Some("RELONE".into());
        a.isin = Some("GB00BBJNQY21".into());
        let mut b = failing_event(2000.0, "ACME 5% 2028 BOND");
        b.transaction_ref = Some("RELTWO".into());
        b.isin = Some("XS2000000001".into());
        let acc = compute_penalty_accruals(&[a, b], &PenaltyRateTable::starter(), "GBP");
        assert_eq!(acc.len(), 2);
        assert_ne!(acc[0].transaction_ref, acc[1].transaction_ref);
    }

    #[test]
    fn reconcile_matches_breaks_and_missing() {
        let computed = vec![
            PenaltyAccrual { transaction_ref: "T1".into(), isin: "AAA".into(), penalty_type: "SEFP".into(), computed_amount: -10.00, direction: "payable".into(), currency: "EUR".into(), ..Default::default() },
            PenaltyAccrual { transaction_ref: "T2".into(), isin: "BBB".into(), penalty_type: "SEFP".into(), computed_amount: -5.00, direction: "payable".into(), currency: "EUR".into(), ..Default::default() },
        ];
        let reported = vec![
            // T1 matches exactly, T2 differs by 0.50 (a break), T3 only reported (missing_computed)
            ReportedPenalty { transaction_ref: "T1".into(), isin: "AAA".into(), penalty_type: "SEFP".into(), reported_amount: -10.00, direction: "payable".into(), currency: "EUR".into(), ..Default::default() },
            ReportedPenalty { transaction_ref: "T2".into(), isin: "BBB".into(), penalty_type: "SEFP".into(), reported_amount: -5.50, direction: "payable".into(), currency: "EUR".into(), ..Default::default() },
            ReportedPenalty { transaction_ref: "T3".into(), isin: "CCC".into(), penalty_type: "SEFP".into(), reported_amount: -3.00, direction: "payable".into(), currency: "EUR".into(), ..Default::default() },
        ];
        let (lines, sum) = reconcile_penalties(&computed, &reported);
        assert_eq!(lines.len(), 3);
        assert_eq!(sum.matched, 1);
        assert_eq!(sum.breaks, 1);
        assert_eq!(sum.missing_computed, 1);
        assert_eq!(sum.missing_reported, 0);
        assert_eq!(sum.computed_total, -15.00);
        assert_eq!(sum.reported_total, -18.50);
        assert_eq!(sum.net_diff, -3.50);
        assert_eq!(sum.break_amount, 3.50); // 0.50 (T2) + 3.00 (T3)
        assert!(!sum.all_reconciled);
    }

    #[test]
    fn reconcile_treats_direction_mismatch_as_a_break_not_a_match() {
        // A computed payable of 10.00 vs a *reported receivable* of 10.00 is a
        // genuine break (the CSD says we're owed; we computed that we owe) —
        // the unsigned magnitudes are equal, so this only surfaces as a break
        // if the amounts are signed by direction rather than compared raw.
        let computed = vec![PenaltyAccrual {
            transaction_ref: "T1".into(),
            isin: "AAA".into(),
            penalty_type: "SEFP".into(),
            computed_amount: -10.00,
            direction: "payable".into(),
            currency: "EUR".into(),
            ..Default::default()
        }];
        let reported = vec![ReportedPenalty {
            transaction_ref: "T1".into(),
            isin: "AAA".into(),
            penalty_type: "SEFP".into(),
            reported_amount: 10.00,
            direction: "receivable".into(),
            currency: "EUR".into(),
            ..Default::default()
        }];
        let (lines, sum) = reconcile_penalties(&computed, &reported);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].status, "break");
        assert_eq!(sum.matched, 0);
        assert_eq!(sum.breaks, 1);
        assert_eq!(sum.net_diff, 20.00, "reported (+10) - computed (-10) = 20, the true swing");
    }

    #[test]
    fn reconcile_nets_mixed_directions_in_the_summary_totals() {
        let computed = vec![
            PenaltyAccrual { transaction_ref: "T1".into(), isin: "AAA".into(), penalty_type: "SEFP".into(), computed_amount: -10.00, direction: "payable".into(), currency: "EUR".into(), ..Default::default() },
            PenaltyAccrual { transaction_ref: "T2".into(), isin: "BBB".into(), penalty_type: "SEFP".into(), computed_amount: 4.00, direction: "receivable".into(), currency: "EUR".into(), ..Default::default() },
        ];
        let reported = vec![
            ReportedPenalty { transaction_ref: "T1".into(), isin: "AAA".into(), penalty_type: "SEFP".into(), reported_amount: -10.00, direction: "payable".into(), currency: "EUR".into(), ..Default::default() },
            ReportedPenalty { transaction_ref: "T2".into(), isin: "BBB".into(), penalty_type: "SEFP".into(), reported_amount: 4.00, direction: "receivable".into(), currency: "EUR".into(), ..Default::default() },
        ];
        let (_, sum) = reconcile_penalties(&computed, &reported);
        assert_eq!(sum.computed_total, -6.00, "net position: -10 payable + 4 receivable");
        assert_eq!(sum.reported_total, -6.00);
        assert_eq!(sum.net_diff, 0.0);
        assert_eq!(sum.matched, 2);
        assert!(sum.all_reconciled);
    }

    #[test]
    fn parses_penalty_statement_csv() {
        let csv = "penalty_ref,transaction_ref,isin,counterparty,currency,penalty_type,amount,period\n\
                   PEN1,MT537REF1,GB00B03MLX29,BANKGB22,GBP,SEFP,1.23,2026-05\n";
        let r = parse_penalty_statement_csv(csv);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].transaction_ref, "MT537REF1");
        // No direction column → defaults to payable → negative.
        assert_eq!(r[0].reported_amount, -1.23);
        assert_eq!(r[0].direction, "payable");
        assert_eq!(r[0].currency, "GBP");
    }

    #[test]
    fn parses_receivable_direction_as_positive() {
        let csv = "transaction_ref,isin,currency,amount,direction\n\
                   T9,GB00B03MLX29,GBP,12.75,receivable\n";
        let r = parse_penalty_statement_csv(csv);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].reported_amount, 12.75);
        assert_eq!(r[0].direction, "receivable");
    }

    #[test]
    fn parses_quoted_fields_with_embedded_commas() {
        // Instrument descriptions and counterparty names routinely contain
        // commas; a naive split(',') would misparse and shift every column
        // after the quoted one.
        let csv = "penalty_ref,transaction_ref,isin,counterparty,currency,penalty_type,amount,period,direction\n\
                   PEN9,REF9,GB00B03MLX29,\"Big Bank, London Branch\",GBP,SEFP,3.45,2026-05,payable\n";
        let r = parse_penalty_statement_csv(csv);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].counterparty_bic.as_deref(), Some("Big Bank, London Branch"));
        assert_eq!(r[0].currency, "GBP", "the comma inside the quoted field must not shift later columns");
        assert_eq!(r[0].reported_amount, -3.45);
    }

    #[test]
    fn unrecognised_penalty_type_defaults_to_sefp() {
        let csv = "transaction_ref,isin,amount,penalty_type\nT1,AAA,1.00,BOGUS\n";
        let r = parse_penalty_statement_csv(csv);
        assert_eq!(r[0].penalty_type, "SEFP");
    }

    // The wedge, end to end: raw MT537 .fin → normalized event → penalty accrual.
    #[test]
    fn mt537_fixture_produces_a_penalty_accrual() {
        use swift_core::parse_message;
        use swift_db::{materialize_message, InboundMessage};
        use swift_schema::SchemaCatalog;

        let fin = include_str!("../../../examples/mt537_sample.fin");
        let schema = include_str!("../../../examples/schemas/mt537.yaml");
        let catalog = SchemaCatalog::from_yaml_str(schema).expect("load mt537 schema");
        let inbound = InboundMessage { id: "MT537REF1".into(), message_type: "MT537".into(), body: fin.into() };
        let parsed = parse_message(inbound.body.as_bytes());
        let batch = materialize_message(&catalog, &inbound, &parsed).expect("materialize");
        let events = swift_normalize::normalize(&batch);

        let accruals = compute_penalty_accruals(&events, &PenaltyRateTable::starter(), "GBP");
        assert_eq!(accruals.len(), 1, "the MT537 pending transaction accrues one penalty");
        let a = &accruals[0];
        assert_eq!(a.transaction_ref, "RELREF1", "the :20C::RELA transaction-scope reference, not the message id");
        assert_eq!(a.isin, "GB00B03MLX29");
        assert_eq!(a.reference_amount, 12_345.67, "the :19A::PSTA posting amount is the penalty base");
        assert_eq!(a.status, "PEND");
        assert_eq!(a.penalty_type, "SEFP");
        // liquid share 1.0 bps of 12,345.67 = 1.23, payable → negative.
        assert_eq!(a.computed_amount, -1.23);
    }

    // The multi-transaction wedge: one MT537 message, two pending
    // transactions, each keyed by its own :20C::RELA reference — this is
    // exactly the case that used to collapse into one summed accrual when
    // accruals keyed on message_id instead of transaction_ref.
    #[test]
    fn mt537_two_fails_fixture_produces_two_distinct_accruals() {
        use swift_core::parse_message;
        use swift_db::{materialize_message, InboundMessage};
        use swift_schema::SchemaCatalog;

        let fin = include_str!("../../../examples/mt537_two_fails.fin");
        let schema = include_str!("../../../examples/schemas/mt537.yaml");
        let catalog = SchemaCatalog::from_yaml_str(schema).expect("load mt537 schema");
        let inbound = InboundMessage { id: "MT537TWOF".into(), message_type: "MT537".into(), body: fin.into() };
        let parsed = parse_message(inbound.body.as_bytes());
        let batch = materialize_message(&catalog, &inbound, &parsed).expect("materialize");
        let events = swift_normalize::normalize(&batch);

        let accruals = compute_penalty_accruals(&events, &PenaltyRateTable::starter(), "GBP");
        assert_eq!(accruals.len(), 2, "two pending transactions in one MT537 message accrue two penalties");

        let gilt = accruals.iter().find(|a| a.isin == "GB00BBJNQY21").expect("gilt accrual");
        let bond = accruals.iter().find(|a| a.isin == "XS2000000001").expect("bond accrual");

        assert_ne!(gilt.transaction_ref, bond.transaction_ref, "each transaction keeps its own reference");
        assert_eq!(gilt.transaction_ref, "RELONE");
        assert_eq!(bond.transaction_ref, "RELTWO");

        assert_eq!(gilt.instrument_type, "sovereign_bond", "UK GILT description classifies as sovereign");
        assert_eq!(bond.instrument_type, "corporate_bond", "% coupon bond description classifies as corporate");

        assert_eq!(gilt.reference_amount, 5_000_000.00);
        assert_eq!(bond.reference_amount, 2_000_000.00);

        // sovereign_bond 0.10 bps of 5,000,000 = 50.00, payable → negative.
        assert_eq!(gilt.computed_amount, -50.00);
        // corporate_bond 0.20 bps of 2,000,000 = 40.00, payable → negative.
        assert_eq!(bond.computed_amount, -40.00);
    }
}
