//! The **SWIFT** source adapter — one implementation of the platform's
//! source-agnostic ingestion contract ([`ingest_core`]). It maps swiftpipe's
//! `materialize_message` output (the generic `settlement_*` rows) into normalized
//! [`SecurityEvent`]s, sub-parsing the ISIN from the raw `:35B:` blob swiftpipe
//! leaves as text. The normalized model + trait live in `ingest-core`; SWIFT
//! knows nothing the other adapters (tabular/FIX/API) don't also produce.
//!
//! Scope: securities settlement / holdings / trade-confirmation flows (MT535,
//! MT536-538, MT540-548, MT515). Resilient scan-by-meaning (not exact column
//! names). Out of scope (net-new in swiftpipe): cash (MT940/942/950), holdings
//! *balance* semantics, MX/ISO20022.

use ingest_core::{extract_isin, EventKind, SecurityEvent, SecuritySource};
use swift_db::{NormalizedRow, ParsedOutputBatch};

// Re-export the shared model so existing consumers of this crate keep working.
pub use ingest_core::{is_isin, EventKind as Kind, SecurityEvent as Event};
pub use ingest_core::extract_isin as extract_isin_blob;

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

/// The SWIFT source adapter.
pub struct SwiftSource;

impl SecuritySource for SwiftSource {
    type Input<'a> = &'a ParsedOutputBatch;
    fn source_id(&self) -> &'static str {
        "swift"
    }
    fn ingest(&self, batch: Self::Input<'_>) -> Vec<SecurityEvent> {
        normalize(batch)
    }
}

/// Map a materialized SWIFT message batch into normalized securities events (one
/// per message; multi-holding sequence grouping is a follow-on).
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
            let (isin, instrument_desc) = scan_isin(&rows);
            Some(SecurityEvent {
                source: "swift".to_string(),
                message_id: msg.message_id.clone(),
                message_type: msg.message_type.clone(),
                kind,
                isin,
                instrument_desc,
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

#[cfg(test)]
mod tests {
    use super::*;
    use swift_core::parse_message;
    use swift_db::{materialize_message, InboundMessage};
    use swift_schema::SchemaCatalog;

    const MT535_FIN: &str = include_str!("../../../examples/mt535_sample.fin");
    const MT535_SCHEMA: &str = include_str!("../../../examples/schemas/mt535.yaml");

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

        // via the trait, to exercise the source-agnostic contract
        let events = SwiftSource.ingest(&batch);
        assert_eq!(events.len(), 1);
        let e = &events[0];
        assert_eq!(e.source, "swift");
        assert_eq!(e.kind, EventKind::Holding);
        assert_eq!(e.isin.as_deref(), Some("GB00B03MLX29"));
        assert_eq!(e.quantity, Some(1000.0));
        assert!(e.settlement_date.is_some());
        assert!(e.safekeeping_account.is_some());
    }
}
