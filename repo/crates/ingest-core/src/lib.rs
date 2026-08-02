//! Source-agnostic normalized ingestion model for the finance platform (ADR-0001).
//!
//! This crate is the platform's ingestion **contract** — the normalized
//! [`SecurityEvent`] and the [`SecuritySource`] trait — with **no dependency on
//! any source format**. SWIFT (the `swift-normalize` crate) is *one*
//! implementation; custodian CSV/Excel (`ingest-tabular`), FIX drop-copy, and
//! OMS/API feeds are others. Keeping this crate source-free is exactly what
//! makes "SWIFT is only one way to ingest this data" true structurally, not by
//! convention.
//!
//! (Housed in the swiftpipe workspace for now; a candidate to relocate to a
//! shared platform crate once a neutral workspace exists — nothing here depends
//! on swiftpipe.)

use serde::Serialize;

/// What a message/record means for the book.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// A statement of holdings (a position snapshot).
    Holding,
    /// A settlement / transaction record.
    Settlement,
    /// A trade confirmation.
    TradeConfirm,
    #[default]
    Other,
}

impl EventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EventKind::Holding => "holding",
            EventKind::Settlement => "settlement",
            EventKind::TradeConfirm => "trade_confirm",
            EventKind::Other => "other",
        }
    }
}

/// A normalized securities event — the platform's read of one custodian/OMS
/// record, independent of the wire format it arrived in.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct SecurityEvent {
    /// Which source produced this (`"swift"`, `"tabular"`, …) — provenance.
    pub source: String,
    pub message_id: String,
    /// The per-transaction reference a CSD penalty statement keys on (MT537
    /// `:20C::RELA` / `:20C::SEME`). Unlike `message_id`, which identifies the
    /// owning message, this identifies the individual transaction inside it —
    /// a message can carry more than one.
    pub transaction_ref: Option<String>,
    pub message_type: String,
    pub kind: EventKind,
    pub isin: Option<String>,
    pub instrument_desc: Option<String>,
    pub quantity: Option<f64>,
    /// Posting / settlement amount of the transaction, when reported (e.g. an
    /// MT537 `:19A::PSTA` posting amount). This is the base a CSDR cash penalty
    /// is computed against. `None` for holdings and records with no money leg.
    pub amount: Option<f64>,
    /// Settlement currency of `amount`, when resolvable.
    pub currency: Option<String>,
    pub settlement_date: Option<String>,
    pub safekeeping_account: Option<String>,
    pub party_bic: Option<String>,
    /// Processing / settlement status of the record, when the source reports one
    /// (e.g. an MT537 *Statement of Pending Transactions* carries a per-status
    /// code such as `PEND`/`PENF`; custodian files may carry a `status` column).
    /// `None` for sources/messages that don't express a status.
    pub status: Option<String>,
}

/// A source adapter turns source-specific input into normalized [`SecurityEvent`]s.
/// SWIFT, tabular custodian files, FIX, and API feeds each implement this; the
/// platform consumes only `SecurityEvent`, never the source format.
pub trait SecuritySource {
    /// The source-specific input this adapter ingests (a parsed SWIFT batch, a
    /// CSV string, a FIX message, …).
    type Input<'a>;
    /// Stable identifier for the source (provenance / diagnostics).
    fn source_id(&self) -> &'static str;
    /// Ingest one unit of source input into normalized events.
    fn ingest(&self, input: Self::Input<'_>) -> Vec<SecurityEvent>;
}

// ---------------------------------------------------------------------------
// Cash (bank/custody account statements) — the second normalized domain.
// Source-agnostic: MT940 (swift-mt940), ISO20022 camt.053 (mx-camt), and bank
// APIs all produce these.
// ---------------------------------------------------------------------------

/// Direction of a cash movement / balance. Reversal variants carry the sign of
/// the movement they reverse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    #[default]
    Credit,
    Debit,
    /// Reversal of a credit (net effect: debit).
    ReversalCredit,
    /// Reversal of a debit (net effect: credit).
    ReversalDebit,
}

impl Direction {
    /// Multiplier for the effect on the account balance (credit +, debit −).
    pub fn sign(self) -> f64 {
        match self {
            Direction::Credit | Direction::ReversalDebit => 1.0,
            Direction::Debit | Direction::ReversalCredit => -1.0,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Direction::Credit => "credit",
            Direction::Debit => "debit",
            Direction::ReversalCredit => "reversal_credit",
            Direction::ReversalDebit => "reversal_debit",
        }
    }
}

/// A statement balance (opening / closing / available).
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Balance {
    pub direction: Direction,
    pub date: Option<String>,
    pub currency: Option<String>,
    pub amount: f64,
}

impl Balance {
    /// Signed balance amount (credit +, debit −).
    pub fn signed(&self) -> f64 {
        self.direction.sign() * self.amount
    }
}

/// One cash movement on an account statement.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct CashEntry {
    pub value_date: Option<String>,
    pub entry_date: Option<String>,
    pub direction: Direction,
    /// Absolute amount as stated.
    pub amount: f64,
    /// Effect on the balance (credit +, debit −) — what P&L/recon sums.
    pub signed_amount: f64,
    /// Bank transaction type code (e.g. TRF, CHG, DIV, INT).
    pub transaction_type: Option<String>,
    pub customer_ref: Option<String>,
    pub bank_ref: Option<String>,
    pub info: Option<String>,
}

/// A normalized cash account statement — the platform's read of one MT940 /
/// camt.053 / bank-API statement, independent of wire format.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct CashStatement {
    pub source: String,
    pub message_id: String,
    pub message_type: String,
    pub account: Option<String>,
    pub statement_number: Option<String>,
    pub currency: Option<String>,
    pub opening_balance: Option<Balance>,
    pub closing_balance: Option<Balance>,
    pub available_balance: Option<Balance>,
    pub entries: Vec<CashEntry>,
}

impl CashStatement {
    /// Net cash movement over the statement (Σ signed entries).
    pub fn net_movement(&self) -> f64 {
        self.entries.iter().map(|e| e.signed_amount).sum()
    }

    /// True if the entries reconcile: opening + Σ movements == closing (within
    /// a cent). Returns `None` when either balance is absent.
    pub fn reconciles(&self) -> Option<bool> {
        let ob = self.opening_balance.as_ref()?.signed();
        let cb = self.closing_balance.as_ref()?.signed();
        Some((ob + self.net_movement() - cb).abs() < 0.005)
    }
}

/// A source adapter that produces normalized [`CashStatement`]s (MT940, camt.053,
/// bank APIs). The cash-domain twin of [`SecuritySource`].
pub trait CashSource {
    type Input<'a>;
    fn source_id(&self) -> &'static str;
    fn ingest(&self, input: Self::Input<'_>) -> Vec<CashStatement>;
}

/// Extract an ISIN from a raw instrument blob like
/// `"ISIN GB00B03MLX29\nACME PLC ORD"`. Returns `(isin, description)`. An ISIN is
/// `ISIN ` followed by a 12-character alphanumeric code (2 leading letters).
pub fn extract_isin(blob: &str) -> (Option<String>, Option<String>) {
    let mut isin = None;
    let mut desc: Vec<&str> = Vec::new();
    for line in blob.split(['\n', '\r']) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if isin.is_none() {
            if let Some(code) = line.strip_prefix("ISIN ").map(str::trim) {
                if is_isin(code) {
                    isin = Some(code.to_string());
                    continue;
                }
            }
        }
        desc.push(line);
    }
    let desc = if desc.is_empty() {
        None
    } else {
        Some(desc.join(" "))
    };
    (isin, desc)
}

/// True if `s` is a syntactically valid ISIN (12 chars, 2 leading letters, all
/// alphanumeric). Does not verify the check digit.
pub fn is_isin(s: &str) -> bool {
    s.len() == 12
        && s.is_ascii()
        && s.bytes().all(|b| b.is_ascii_alphanumeric())
        && s.bytes().take(2).all(|b| b.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_isin_from_blob() {
        let (isin, desc) = extract_isin("ISIN GB00B03MLX29\nACME PLC ORD");
        assert_eq!(isin.as_deref(), Some("GB00B03MLX29"));
        assert_eq!(desc.as_deref(), Some("ACME PLC ORD"));
    }

    #[test]
    fn is_isin_validates() {
        assert!(is_isin("US0378331005"));
        assert!(!is_isin("ACME PLC ORD"));
        assert!(!is_isin("GB00B03MLX2")); // 11 chars
    }
}
