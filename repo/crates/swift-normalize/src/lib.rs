//! Normalize swiftpipe's parsed SWIFT securities messages into platform events.
//!
//! This is the **source-adapter seam** of the finance platform (ADR-0001):
//! swiftpipe is the first *source*, and this crate maps its output — the generic
//! `settlement_*` rows from [`swift_db::materialize_message`] — into typed
//! [`SecurityEvent`]s the P&L/risk engine can consume, extracting the ISIN from
//! the raw `:35B:` instrument blob (which swiftpipe leaves as text).
//!
//! Scope: securities settlement / holdings / trade-confirmation flows (MT535,
//! MT536-538, MT540-548, MT515). The mapping is intentionally resilient — it
//! scans the normalized rows by meaning rather than assuming exact column names,
//! so schema tweaks don't break it. Deliberately out of scope (net-new in
//! swiftpipe, tracked as follow-ons): cash statements (MT940/942/950), holdings
//! *balance* semantics (aggregate/available/blocked quantities, market value),
//! and MX/ISO20022.

use serde::Serialize;
use swift_db::{NormalizedRow, ParsedOutputBatch};

/// What a message means, derived from its type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// MT535 — statement of holdings.
    Holding,
    /// MT536-538, MT540-548 — statement of transactions / settlement.
    Settlement,
    /// MT515/518 — trade confirmation.
    TradeConfirm,
    #[default]
    Other,
}

/// Classify a SWIFT message type into a platform event kind.
pub fn classify(message_type: &str) -> EventKind {
    match message_type.trim().to_ascii_uppercase().as_str() {
        "MT535" => EventKind::Holding,
        "MT536" | "MT537" | "MT538" | "MT540" | "MT541" | "MT542" | "MT543" | "MT544" | "MT545"
        | "MT546" | "MT547" | "MT548" => EventKind::Settlement,
        "MT515" | "MT518" => EventKind::TradeConfirm,
        _ => EventKind::Other,
    }
}

/// A normalized securities event — the platform's read of one custodian message.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct SecurityEvent {
    pub message_id: String,
    pub message_type: String,
    pub kind: EventKind,
    /// ISIN extracted from the `:35B:` instrument blob (when present).
    pub isin: Option<String>,
    /// The remaining instrument description text.
    pub instrument_desc: Option<String>,
    pub quantity: Option<f64>,
    pub settlement_date: Option<String>,
    pub safekeeping_account: Option<String>,
    pub party_bic: Option<String>,
}

/// Map a materialized message batch into normalized securities events (one per
/// message in the batch; multi-holding sequence grouping is a follow-on).
pub fn normalize(batch: &ParsedOutputBatch) -> Vec<SecurityEvent> {
    batch
        .raw_messages
        .iter()
        .filter_map(|msg| {
            let kind = classify(&msg.message_type);
            if kind == EventKind::Other {
                return None;
            }
            let rows: Vec<&NormalizedRow> = batch.normalized_rows.iter().collect();
            Some(SecurityEvent {
                message_id: msg.message_id.clone(),
                message_type: msg.message_type.clone(),
                kind,
                isin: scan_isin(&rows).0,
                instrument_desc: scan_isin(&rows).1,
                quantity: scan_column(&rows, |t, c| t == "settlement_quantity" && c == "quantity")
                    .or_else(|| scan_column(&rows, |_, c| c == "quantity"))
                    .and_then(|v| v.parse::<f64>().ok()),
                settlement_date: scan_column(&rows, |t, c| t == "settlement_trade" && c == "settlement_date")
                    .or_else(|| scan_column(&rows, |_, c| c.contains("settlement") && c.contains("date")))
                    .or_else(|| scan_column(&rows, |_, c| c.contains("date"))),
                safekeeping_account: scan_column(&rows, |_, c| c.contains("safekeep"))
                    .or_else(|| scan_column(&rows, |t, c| t == "settlement_account" && c.contains("account"))),
                party_bic: scan_column(&rows, |t, c| t == "settlement_party" && c == "party")
                    .or_else(|| scan_column(&rows, |_, c| c == "party")),
            })
        })
        .collect()
}

/// First value across the rows whose (table, column) satisfies `pred`.
fn scan_column(rows: &[&NormalizedRow], pred: impl Fn(&str, &str) -> bool) -> Option<String> {
    for row in rows {
        for (col, val) in &row.values {
            if !val.is_empty() && pred(row.table.as_str(), col.as_str()) {
                return Some(val.clone());
            }
        }
    }
    None
}

/// First value across the rows that carries an ISIN; returns (isin, description).
fn scan_isin(rows: &[&NormalizedRow]) -> (Option<String>, Option<String>) {
    for row in rows {
        for val in row.values.values() {
            let (isin, desc) = extract_isin(val);
            if isin.is_some() {
                return (isin, desc);
            }
        }
    }
    (None, None)
}

/// Extract an ISIN from a raw `:35B:` blob like `"ISIN GB00B03MLX29\nACME PLC ORD"`.
/// Returns `(isin, description)`. An ISIN is `ISIN ` followed by a 12-character
/// alphanumeric code (2 leading letters).
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

fn is_isin(s: &str) -> bool {
    s.len() == 12
        && s.is_ascii()
        && s.bytes().all(|b| b.is_ascii_alphanumeric())
        && s.bytes().take(2).all(|b| b.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;
    use swift_core::parse_message;
    use swift_db::{materialize_message, InboundMessage};
    use swift_schema::SchemaCatalog;

    const MT535_FIN: &str = include_str!("../../../examples/mt535_sample.fin");
    const MT535_SCHEMA: &str = include_str!("../../../examples/schemas/mt535.yaml");

    #[test]
    fn extract_isin_from_35b_blob() {
        let (isin, desc) = extract_isin("ISIN GB00B03MLX29\nACME PLC ORD");
        assert_eq!(isin.as_deref(), Some("GB00B03MLX29"));
        assert_eq!(desc.as_deref(), Some("ACME PLC ORD"));
    }

    #[test]
    fn is_isin_rejects_non_isin() {
        assert!(!is_isin("ACME PLC ORD"));
        assert!(!is_isin("GB00B03MLX2")); // 11 chars
        assert!(is_isin("US0378331005"));
    }

    #[test]
    fn classify_message_types() {
        assert_eq!(classify("MT535"), EventKind::Holding);
        assert_eq!(classify("mt542"), EventKind::Settlement);
        assert_eq!(classify("MT515"), EventKind::TradeConfirm);
        assert_eq!(classify("MT999"), EventKind::Other);
    }

    #[test]
    fn maps_real_mt535_fixture_to_holding_event() {
        let catalog = SchemaCatalog::from_yaml_str(MT535_SCHEMA).expect("load mt535 schema");
        catalog.validate().expect("valid schema");
        let inbound = InboundMessage {
            id: "msg-535".into(),
            message_type: "MT535".into(),
            body: MT535_FIN.into(),
        };
        let parsed = parse_message(inbound.body.as_bytes());
        let batch = materialize_message(&catalog, &inbound, &parsed).expect("materialize");

        let events = normalize(&batch);
        assert_eq!(events.len(), 1, "one holding event for the single-holding fixture");
        let e = &events[0];
        assert_eq!(e.kind, EventKind::Holding);
        // The wedge's core: ISIN extracted from the raw :35B: blob, quantity parsed.
        assert_eq!(e.isin.as_deref(), Some("GB00B03MLX29"));
        assert_eq!(e.quantity, Some(1000.0));
        // Present-but-not-brittle (exact values depend on schema column choices).
        assert!(e.settlement_date.is_some(), "settlement date populated");
        assert!(e.safekeeping_account.is_some(), "safekeeping account populated");
    }
}
