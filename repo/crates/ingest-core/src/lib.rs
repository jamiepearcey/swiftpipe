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

/// A normalized securities event — the platform's read of one custodian/OMS
/// record, independent of the wire format it arrived in.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct SecurityEvent {
    /// Which source produced this (`"swift"`, `"tabular"`, …) — provenance.
    pub source: String,
    pub message_id: String,
    pub message_type: String,
    pub kind: EventKind,
    pub isin: Option<String>,
    pub instrument_desc: Option<String>,
    pub quantity: Option<f64>,
    pub settlement_date: Option<String>,
    pub safekeeping_account: Option<String>,
    pub party_bic: Option<String>,
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
    let desc = if desc.is_empty() { None } else { Some(desc.join(" ")) };
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
