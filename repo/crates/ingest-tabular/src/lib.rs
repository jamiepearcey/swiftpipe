//! Tabular (custodian CSV/Excel-export) source adapter — a **non-SWIFT** path
//! into the same normalized model, proving [`ingest_core::SecurityEvent`] is
//! source-agnostic. Many custodians deliver positions as delimited files rather
//! than SWIFT; this maps such a positions file into the exact same events the
//! SWIFT adapter produces.
//!
//! Input: a CSV with a header row. Recognized columns (case-insensitive, any
//! order, extras ignored): `isin`, `quantity`, `account`, `settlement_date`,
//! `party`, `id`, `description`. No external CSV dependency — simple, quoted
//! fields are not required for custodian position dumps.

use ingest_core::{is_isin, EventKind, SecurityEvent, SecuritySource};

/// The tabular source adapter.
pub struct TabularSource;

impl SecuritySource for TabularSource {
    type Input<'a> = &'a str;
    fn source_id(&self) -> &'static str {
        "tabular"
    }
    fn ingest(&self, csv: Self::Input<'_>) -> Vec<SecurityEvent> {
        parse_csv(csv)
    }
}

/// Parse a custodian positions CSV into normalized holding events.
pub fn parse_csv(csv: &str) -> Vec<SecurityEvent> {
    let mut rows = csv.lines().filter(|l| !l.trim().is_empty());
    let header: Vec<String> = match rows.next() {
        Some(h) => h.split(',').map(|c| c.trim().to_ascii_lowercase()).collect(),
        None => return Vec::new(),
    };
    let idx = |name: &str| header.iter().position(|h| h == name);
    let (i_isin, i_qty, i_acct, i_date, i_party, i_id, i_desc) = (
        idx("isin"),
        idx("quantity"),
        idx("account"),
        idx("settlement_date"),
        idx("party"),
        idx("id"),
        idx("description"),
    );

    rows.enumerate()
        .map(|(row_no, line)| {
            let cells: Vec<&str> = line.split(',').map(str::trim).collect();
            let get = |i: Option<usize>| {
                i.and_then(|i| cells.get(i))
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty())
            };
            SecurityEvent {
                source: "tabular".to_string(),
                message_id: get(i_id).unwrap_or_else(|| format!("row-{}", row_no + 1)),
                message_type: "CSV_POSITIONS".to_string(),
                kind: EventKind::Holding,
                isin: get(i_isin).filter(|s| is_isin(s)),
                instrument_desc: get(i_desc),
                quantity: get(i_qty).and_then(|q| q.parse::<f64>().ok()),
                settlement_date: get(i_date),
                safekeeping_account: get(i_acct),
                party_bic: get(i_party),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_custodian_positions_csv() {
        let csv = "isin,quantity,account,settlement_date,party\n\
                   GB00B03MLX29,1000,SAFE535,2026-05-13,BANKGB22XXX\n\
                   US0378331005,250.5,SAFE900,2026-05-14,BANKUS33XXX\n";
        let events = parse_csv(csv);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].source, "tabular");
        assert_eq!(events[0].kind, EventKind::Holding);
        assert_eq!(events[0].isin.as_deref(), Some("GB00B03MLX29"));
        assert_eq!(events[0].quantity, Some(1000.0));
        assert_eq!(events[0].safekeeping_account.as_deref(), Some("SAFE535"));
        assert_eq!(events[1].quantity, Some(250.5));
    }

    #[test]
    fn invalid_isin_is_dropped_not_faked() {
        let events = parse_csv("isin,quantity\nNOTANISIN,10\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].isin, None);
        assert_eq!(events[0].quantity, Some(10.0));
    }

    #[test]
    fn adapter_via_trait_produces_same_event_type() {
        // The whole point: a non-SWIFT source, same SecurityEvent contract.
        let s = TabularSource;
        assert_eq!(s.source_id(), "tabular");
        let ev = s.ingest("isin,quantity\nUS0378331005,10\n");
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].isin.as_deref(), Some("US0378331005"));
    }
}
