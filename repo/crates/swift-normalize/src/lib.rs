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
use swift_db::{FieldRow, NormalizedRow, ParsedOutputBatch, RawMessageRow};

// Re-export the shared model so existing consumers of this crate keep working.
pub use ingest_core::extract_isin as extract_isin_blob;
pub use ingest_core::{is_isin, EventKind as Kind, SecurityEvent as Event};

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

/// Map a materialized SWIFT message batch into normalized securities events.
///
/// A message can carry more than one transaction (e.g. an MT537 *Statement of
/// Pending Transactions* with several `:16R:TRAN` blocks); each transaction
/// gets its own [`SecurityEvent`], not just the message's first one. A
/// transaction is identified by the row that yields an ISIN (`scan_isin`'s
/// per-row match) — its `sequence_path` is that transaction's scope. Fields
/// are then resolved preferring rows in the same branch as that scope (an
/// ancestor, a descendant, a sibling under the same immediate parent — e.g.
/// MT537's `:20C::RELA` LINK block sits beside, not inside, TRANSDET — or a
/// same-occurrence "cousin" under an unrelated top-level sequence — e.g.
/// MT536's `SETDET[N]/AMT` settlement amount sits beside, not inside, the
/// `TRADDET[N]` instrument; see `in_branch`), nearest match first, then
/// root-scope (`"$"`) rows for message-level context. Messages with no
/// ISIN-bearing row at all (no identifiable transaction) fall back to one
/// event per message, scanning the whole message unscoped — today's original
/// behavior.
pub fn normalize(batch: &ParsedOutputBatch) -> Vec<SecurityEvent> {
    batch
        .raw_messages
        .iter()
        .flat_map(|msg| normalize_message(batch, msg))
        .collect()
}

fn normalize_message(batch: &ParsedOutputBatch, msg: &RawMessageRow) -> Vec<SecurityEvent> {
    let kind = classify(&msg.message_type);
    if kind == EventKind::Other {
        return Vec::new();
    }

    // Rows belonging to *this* message only — a multi-message batch must not
    // let one message's data leak into another's events.
    let rows: Vec<&NormalizedRow> = batch
        .normalized_rows
        .iter()
        .filter(|row| {
            row.values.get("message_id").map(String::as_str) == Some(msg.message_id.as_str())
        })
        .collect();

    // Every row that yields an ISIN is one transaction occurrence; its
    // sequence_path is that transaction's scope.
    let instruments: Vec<(String, Option<String>, Option<String>)> = rows
        .iter()
        .filter_map(|row| {
            let path = row.values.get("sequence_path")?;
            row.values.values().find_map(|val| {
                let (isin, desc) = extract_isin(val);
                isin.map(|isin| (path.clone(), Some(isin), desc))
            })
        })
        .collect();

    if instruments.is_empty() {
        // No identifiable transaction — keep the original one-event-per-message
        // behavior, scanning the whole message unscoped.
        return vec![build_event(batch, msg, kind, &rows, None, None, None)];
    }

    instruments
        .into_iter()
        .map(|(path, isin, instrument_desc)| {
            build_event(
                batch,
                msg,
                kind,
                &rows,
                Some(path.as_str()),
                isin,
                instrument_desc,
            )
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn build_event(
    batch: &ParsedOutputBatch,
    msg: &RawMessageRow,
    kind: EventKind,
    rows: &[&NormalizedRow],
    instrument_path: Option<&str>,
    isin: Option<String>,
    instrument_desc: Option<String>,
) -> SecurityEvent {
    let amount_field = find_field(rows, instrument_path, |t, c| {
        t.contains("transaction") && c == "amount"
    })
    .or_else(|| find_field(rows, instrument_path, |_, c| c == "amount"));
    let amount = amount_field
        .and_then(|(row, col)| row.values.get(col))
        .and_then(|v| v.parse::<f64>().ok());
    // Currency is stripped from the amount at the materialize layer; recover
    // the ISO 4217 prefix from the amount's render payload column (the raw,
    // pre-normalization capture, e.g. "GBP5000000,00").
    let currency = amount_field.and_then(|(row, col)| {
        let render_prefix = format!("{col}__render_");
        row.values
            .iter()
            .find(|(k, _)| k.starts_with(&render_prefix))
            .and_then(|(_, v)| extract_currency_prefix(v))
    });

    SecurityEvent {
        source: "swift".to_string(),
        message_id: msg.message_id.clone(),
        transaction_ref: scan_transaction_ref(rows, instrument_path),
        message_type: msg.message_type.clone(),
        kind,
        isin,
        instrument_desc,
        quantity: scan_value(rows, instrument_path, |t, c| {
            t == "settlement_quantity" && c == "quantity"
        })
        .or_else(|| scan_value(rows, instrument_path, |_, c| c == "quantity"))
        .and_then(|v| v.parse::<f64>().ok()),
        // Posting / settlement amount — the CSDR penalty base. MT537 keeps
        // it under `mt537_transaction.amount` (:19A::PSTA); prefer that over
        // any reported-penalty amounts in a PENA block.
        amount,
        currency,
        // ISD (Intended Settlement Date) is the legal basis for CSDR penalty
        // accrual, so this must be the SETT-qualified date, never a
        // statement-level one. Selected by qualifier (`FieldRow::qualifier`,
        // populated straight off the wire), not by column name — column names
        // vary per message type (MT535: `settlement_date`, MT537: `date`) and
        // a name-based match previously collided with `mt537_statement`'s
        // `:98A::STAT` statement date.
        settlement_date: scan_settlement_date(&batch.fields, &msg.message_id, instrument_path),
        safekeeping_account: scan_value(rows, instrument_path, |_, c| c.contains("safekeep"))
            .or_else(|| scan_value(rows, instrument_path, |t, c| t == "settlement_account" && c.contains("account")))
            // MT537 keeps the safekeeping account under an `*account` entity
            // with a plain `account` column — either the statement-level
            // account (`mt537_account`) or, for a specific transaction leg,
            // its settlement party's account (`mt537_settlement_party`).
            .or_else(|| {
                scan_value(rows, instrument_path, |t, c| c == "account" && (t.contains("account") || t.contains("party")))
            }),
        party_bic: scan_value(rows, instrument_path, |t, c| {
            t == "settlement_party" && c == "party"
        })
        .or_else(|| scan_value(rows, instrument_path, |_, c| c == "party")),
        // Statement-of-status messages (MT537 pending transactions) report a
        // per-transaction status code (:25D::IPRC//PEND). Other securities
        // flows have no status, leaving this `None`.
        status: scan_value(rows, instrument_path, |t, c| {
            c == "status_code" && t.contains("status")
        })
        .or_else(|| scan_value(rows, instrument_path, |_, c| c == "status_code")),
    }
}

/// The per-transaction reference a CSD penalty statement keys on: prefer the
/// transaction-scope linked reference (MT537 `:20C::RELA`, entity
/// `mt537_transaction_link`/`linked_reference`); else the message-level
/// sender reference (`:20C::SEME`, entity `mt537_statement`/`sender_reference`,
/// searched across the whole message since it isn't transaction-scoped);
/// else `None`.
fn scan_transaction_ref(rows: &[&NormalizedRow], instrument_path: Option<&str>) -> Option<String> {
    scan_value(rows, instrument_path, |t, c| {
        t == "mt537_transaction_link" && c == "linked_reference"
    })
    .or_else(|| {
        scan_value(rows, None, |t, c| {
            t == "mt537_statement" && c == "sender_reference"
        })
    })
}

/// The settlement date selected by SWIFT qualifier `SETT` on a date-shaped
/// tag (`98a`), read straight from the parsed fields (`FieldRow::qualifier`
/// is populated off the wire, independent of the entity/column naming a
/// schema happens to choose) — never a statement-level date. Scoped to the
/// transaction branch when one is known; `None` if no SETT-qualified field is
/// present, rather than guessing.
fn scan_settlement_date(
    fields: &[FieldRow],
    message_id: &str,
    instrument_path: Option<&str>,
) -> Option<String> {
    let mut candidates: Vec<&FieldRow> = fields
        .iter()
        .filter(|f| {
            f.message_id == message_id
                && f.qualifier.as_deref() == Some("SETT")
                && f.tag.starts_with("98")
        })
        .collect();

    if let Some(instrument_path) = instrument_path {
        candidates.retain(|f| {
            f.sequence_path
                .as_deref()
                .is_some_and(|path| in_branch(path, instrument_path))
        });
        candidates.sort_by_key(|f| {
            let path = f.sequence_path.as_deref().unwrap_or("");
            std::cmp::Reverse(shared_depth(path, instrument_path))
        });
    }

    candidates
        .first()
        .map(|f| format_swift_date(raw_qualified_value(&f.raw_value)))
}

/// A SWIFT qualified field's raw value is captured whole (e.g. `:SETT//20260512`);
/// strip the `qualifier//` prefix to get the bare value.
fn raw_qualified_value(raw: &str) -> &str {
    raw.rsplit("//").next().unwrap_or(raw)
}

/// Format an 8-digit SWIFT date (`YYYYMMDD`) as `YYYY-MM-DD`; pass through
/// anything else unchanged.
fn format_swift_date(raw: &str) -> String {
    if raw.len() == 8 && raw.bytes().all(|b| b.is_ascii_digit()) {
        format!("{}-{}-{}", &raw[0..4], &raw[4..6], &raw[6..8])
    } else {
        raw.to_string()
    }
}

/// Recover an ISO 4217 currency prefix from a raw (pre-normalization) SWIFT
/// amount payload such as `"GBP5000000,00"`: 3 leading uppercase letters
/// immediately followed by a digit.
fn extract_currency_prefix(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    if bytes.len() > 3 && bytes[..3].iter().all(u8::is_ascii_uppercase) && bytes[3].is_ascii_digit()
    {
        Some(raw[..3].to_string())
    } else {
        None
    }
}

/// First (row, column) pair, in scope order, whose (table, column) satisfies
/// `pred` and carries a non-empty value.
fn find_field<'a>(
    rows: &[&'a NormalizedRow],
    instrument_path: Option<&str>,
    pred: impl Fn(&str, &str) -> bool,
) -> Option<(&'a NormalizedRow, &'a str)> {
    for row in scoped_rows(rows, instrument_path) {
        for (col, val) in &row.values {
            if !val.is_empty() && pred(row.table.as_str(), col.as_str()) {
                return Some((row, col.as_str()));
            }
        }
    }
    None
}

/// First value, in scope order, whose (table, column) satisfies `pred`.
fn scan_value(
    rows: &[&NormalizedRow],
    instrument_path: Option<&str>,
    pred: impl Fn(&str, &str) -> bool,
) -> Option<String> {
    find_field(rows, instrument_path, pred).and_then(|(row, col)| row.values.get(col).cloned())
}

/// Rows in search-priority order: when `instrument_path` is known, rows in
/// the same branch (nearest/deepest match first), then root-scope (`"$"`)
/// rows; when unknown (no identifiable transaction in the message), every
/// row, unscoped — the original message-wide scan.
fn scoped_rows<'a>(
    rows: &[&'a NormalizedRow],
    instrument_path: Option<&str>,
) -> Vec<&'a NormalizedRow> {
    let Some(instrument_path) = instrument_path else {
        return rows.to_vec();
    };
    let mut branch: Vec<&'a NormalizedRow> = rows
        .iter()
        .filter(|row| {
            row.values
                .get("sequence_path")
                .is_some_and(|path| path != "$" && in_branch(path, instrument_path))
        })
        .copied()
        .collect();
    branch.sort_by_key(|row| {
        let path = row
            .values
            .get("sequence_path")
            .map(String::as_str)
            .unwrap_or("");
        std::cmp::Reverse(shared_depth(path, instrument_path))
    });
    let root = rows
        .iter()
        .filter(|row| row.values.get("sequence_path").map(String::as_str) == Some("$"))
        .copied();
    branch.into_iter().chain(root).collect()
}

/// Two sequence paths are in the same branch if:
///
/// 1. One is an ancestor-or-self of the other (a full, occurrence-exact
///    prefix match) — e.g. `TRAN[1]/TRANSDET[0]` is in the branch of
///    `TRAN[1]/TRANSDET[0]/SETPRTY[0]` and vice versa.
/// 2. They're siblings under the same immediate parent *occurrence* — equal
///    depth, and every segment but the last is identical (name *and*
///    occurrence) — e.g. MT537's `TRAN[1]/LINK[0]` and `TRAN[1]/TRANSDET[0]`
///    are siblings under the same `TRAN[1]` occurrence, both relevant to the
///    transaction that occurrence's ISIN identifies. The diverging last
///    segment's own name/occurrence don't matter: the shared parent
///    occurrence already pins the correlation.
/// 3. They're "cousins" — no shared parent at all (they diverge at the very
///    first segment, i.e. different top-level sequences), but that first
///    segment's *occurrence* matches. This is what MT536 needs: the
///    instrument (`TRADDET`, depth 1) and its settlement amount
///    (`SETDET/AMT`, depth 2) are declared as separate top-level repeating
///    sequences, not nested under a shared parent — the schema's only
///    correlation between the Nth `TRADDET` and the Nth `SETDET` is that
///    they're the Nth occurrence of each. Requiring the occurrence to match
///    at the point of divergence is what keeps a second instrument's
///    `SETDET[1]/AMT` from ever being attributed to `TRADDET[0]` (occurrence
///    1 != occurrence 0), and is *also* what closes a latent bug the old
///    "any two same-depth paths are siblings" rule had: `TRADDET[1]`'s own
///    direct fields (depth 1, no parent to distinguish it) used to satisfy
///    the old sibling test against `TRADDET[0]` unconditionally, since
///    stripping the one and only segment of a depth-1 path leaves nothing to
///    compare. That let a second instrument's own fields leak into the
///    first's event for exactly the MT536 shape this function exists to
///    scope. Case 3 subsumes and fixes that: at a depth-1 divergence,
///    occurrence equality is checked instead of assumed.
///
/// Different occurrences of a repeating ancestor/sibling/cousin
/// (`TRAN[0]` vs `TRAN[1]`, `TRADDET[0]` vs `TRADDET[1]`, `SETDET[0]/AMT` vs
/// `TRADDET[1]`) never match, which is what keeps a multi-transaction or
/// multi-instrument message's events from cross-contaminating.
fn in_branch(candidate: &str, instrument: &str) -> bool {
    let c = path_segments(candidate);
    let i = path_segments(instrument);
    if c.is_empty() || i.is_empty() {
        return false;
    }
    let min_len = c.len().min(i.len());
    if c[..min_len] == i[..min_len] {
        return true; // ancestor-or-self, either direction
    }
    match (0..min_len).find(|&k| c[k] != i[k]) {
        // Diverge immediately: different top-level sequences entirely, no
        // shared parent occurrence to lean on. Only correlated if they're
        // the same occurrence of their respective (possibly differently
        // named) top-level sequence — see case 3 above.
        Some(0) => segment_occurrence(c[0]).is_some_and(|o| Some(o) == segment_occurrence(i[0])),
        // Diverge partway down a shared ancestor occurrence: true siblings,
        // only when at equal depth (the divergence is the final segment of
        // both) — see case 2 above.
        Some(k) => c.len() == i.len() && k == c.len() - 1,
        // Unreachable: a `None` here means the compared prefix was fully
        // equal, which the ancestor-or-self check above already returned on.
        None => false,
    }
}

/// The `[N]` occurrence index embedded in one path segment, e.g.
/// `"TRADDET[2]"` -> `Some("2")`. Compared as text rather than parsed to an
/// integer since all we ever do with it is equality.
fn segment_occurrence(segment: &str) -> Option<&str> {
    let start = segment.rfind('[')?;
    let end = segment.rfind(']')?;
    (end > start).then(|| &segment[start + 1..end])
}

/// Count of leading segments two sequence paths share.
fn shared_depth(a: &str, b: &str) -> usize {
    path_segments(a)
        .iter()
        .zip(path_segments(b).iter())
        .take_while(|(x, y)| x == y)
        .count()
}

fn path_segments(path: &str) -> Vec<&str> {
    if path.is_empty() || path == "$" {
        Vec::new()
    } else {
        path.split('/').collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use swift_core::parse_message;
    use swift_db::{materialize_message, InboundMessage};
    use swift_schema::SchemaCatalog;

    const MT535_FIN: &str = include_str!("../../../examples/mt535_sample.fin");
    const MT535_SCHEMA: &str = include_str!("../../../examples/schemas/mt535.yaml");
    const MT537_FIN: &str = include_str!("../../../examples/mt537_sample.fin");
    const MT537_SCHEMA: &str = include_str!("../../../examples/schemas/mt537.yaml");
    const MT536_SCHEMA: &str = include_str!("../../../examples/schemas/mt536.yaml");
    const MT536_AMOUNT_FIN: &str =
        include_str!("../../../examples/mt536_settlement_amount_sample.fin");
    const MT536_TWO_INSTRUMENTS_FIN: &str =
        include_str!("../../../examples/mt536_two_instruments.fin");

    #[test]
    fn classify_message_types() {
        assert_eq!(classify("MT535"), EventKind::Holding);
        assert_eq!(classify("mt542"), EventKind::Settlement);
        assert_eq!(classify("MT537"), EventKind::Settlement);
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
        assert_eq!(
            e.settlement_date.as_deref(),
            Some("2026-05-13"),
            "must be the :98A::SETT date, not any other date on the message"
        );
        assert!(e.safekeeping_account.is_some());
        // A holding statement is not a status report — no pending status.
        assert_eq!(e.status, None);
    }

    #[test]
    fn maps_real_mt537_pending_transaction_to_settlement_event() {
        // MT537 is a Statement of Pending Transactions — the defining datum is the
        // per-transaction status (:25D::IPRC//PEND). Support means that status
        // survives normalization into the platform's SecurityEvent, alongside the
        // instrument / quantity / settlement date it reports.
        let catalog = SchemaCatalog::from_yaml_str(MT537_SCHEMA).expect("load mt537 schema");
        catalog.validate().expect("valid schema");
        let inbound = InboundMessage {
            id: "msg-537".into(),
            message_type: "MT537".into(),
            body: MT537_FIN.into(),
        };
        let parsed = parse_message(inbound.body.as_bytes());
        let batch = materialize_message(&catalog, &inbound, &parsed).expect("materialize");

        let events = SwiftSource.ingest(&batch);
        assert_eq!(events.len(), 1);
        let e = &events[0];
        assert_eq!(e.source, "swift");
        assert_eq!(e.kind, EventKind::Settlement);
        assert_eq!(e.isin.as_deref(), Some("GB00B03MLX29"));
        assert_eq!(e.quantity, Some(1000.0));
        assert_eq!(
            e.status.as_deref(),
            Some("PEND"),
            "the MT537 pending status must survive normalization"
        );
        assert_eq!(
            e.settlement_date.as_deref(),
            Some("2026-05-12"),
            "must be the :98A::SETT intended settlement date (the CSDR penalty basis), \
             not the :98A::STAT statement date"
        );
        assert_eq!(e.currency.as_deref(), Some("GBP"));
        assert!(e.safekeeping_account.is_some());
    }

    #[test]
    fn maps_mt537_with_two_pending_transactions_to_two_events() {
        // Bug fix: normalize() used to emit one SecurityEvent per raw MESSAGE,
        // scanning the whole batch for the first match of each field — so a
        // statement carrying two pending fails collapsed into one event and
        // silently dropped the second. Each `:16R:TRAN` occurrence (identified
        // by the ISIN-bearing TRANSDET row it contains) must become its own
        // event, with fields resolved from that transaction's own branch.
        const MT537_TWO_FAILS_FIN: &str = include_str!("../../../examples/mt537_two_fails.fin");

        let catalog = SchemaCatalog::from_yaml_str(MT537_SCHEMA).expect("load mt537 schema");
        catalog.validate().expect("valid schema");
        let inbound = InboundMessage {
            id: "msg-537-two".into(),
            message_type: "MT537".into(),
            body: MT537_TWO_FAILS_FIN.into(),
        };
        let parsed = parse_message(inbound.body.as_bytes());
        let batch = materialize_message(&catalog, &inbound, &parsed).expect("materialize");

        let events = SwiftSource.ingest(&batch);
        assert_eq!(
            events.len(),
            2,
            "one event per pending transaction, not one per message"
        );

        let gilt = events
            .iter()
            .find(|e| e.isin.as_deref() == Some("GB00BBJNQY21"))
            .expect("gilt fail present");
        let bond = events
            .iter()
            .find(|e| e.isin.as_deref() == Some("XS2000000001"))
            .expect("bond fail present");

        assert_eq!(gilt.amount, Some(5_000_000.0));
        assert_eq!(bond.amount, Some(2_000_000.0));
        assert_eq!(gilt.currency.as_deref(), Some("GBP"));
        assert_eq!(bond.currency.as_deref(), Some("GBP"));
        assert_eq!(gilt.settlement_date.as_deref(), Some("2026-05-12"));
        assert_eq!(bond.settlement_date.as_deref(), Some("2026-05-12"));
        assert_eq!(gilt.status.as_deref(), Some("PEND"));
        assert_eq!(bond.status.as_deref(), Some("PEND"));

        assert_ne!(
            gilt.transaction_ref, bond.transaction_ref,
            "each transaction keeps its own :20C::RELA reference"
        );
        assert_eq!(gilt.transaction_ref.as_deref(), Some("RELONE"));
        assert_eq!(bond.transaction_ref.as_deref(), Some("RELTWO"));
    }

    #[test]
    fn maps_mt536_settlement_amount_under_sibling_sequence() {
        // Bug fix: MT536 declares its instrument (TRADDET, depth 1) and its
        // settlement amount (SETDET/AMT, depth 2) as separate top-level
        // sequences — SETDET is a sibling of TRADDET, not its parent, so
        // SETDET/AMT is neither an ancestor/descendant of TRADDET nor an
        // equal-depth sibling of it. `in_branch` used to reject it outright,
        // so `SecurityEvent.amount`/`.currency` came back `None` even though
        // the message carries the money (:19A::SETT//EUR125000,00).
        let catalog = SchemaCatalog::from_yaml_str(MT536_SCHEMA).expect("load mt536 schema");
        catalog.validate().expect("valid schema");
        let inbound = InboundMessage {
            id: "msg-536-amt".into(),
            message_type: "MT536".into(),
            body: MT536_AMOUNT_FIN.into(),
        };
        let parsed = parse_message(inbound.body.as_bytes());
        let batch = materialize_message(&catalog, &inbound, &parsed).expect("materialize");

        let events = SwiftSource.ingest(&batch);
        assert_eq!(events.len(), 1);
        let e = &events[0];
        assert_eq!(e.isin.as_deref(), Some("IE00BD1RP616"));
        assert_eq!(
            e.amount,
            Some(125_000.0),
            "SETDET/AMT's settlement amount must reach the event despite sitting \
             beside, not inside, TRADDET"
        );
        assert_eq!(e.currency.as_deref(), Some("EUR"));
    }

    #[test]
    fn maps_mt536_two_instruments_without_cross_attributing_amounts() {
        // Adversarial case for the sibling-sequence fix above: a message
        // with two TRADDET/SETDET pairs (two instruments) must resolve each
        // instrument's own settlement amount, never the other's. A fix that
        // merely relaxes `in_branch`'s depth requirement without also
        // requiring the two sequences to be the *same occurrence* would
        // (re)introduce exactly this cross-contamination — and would, in
        // this schema's flat depth-1 TRADDET encoding, even risk leaking a
        // second instrument's own direct fields into the first's event,
        // which the old "any two same-depth paths are siblings" rule already
        // permitted for depth-1 paths.
        let catalog = SchemaCatalog::from_yaml_str(MT536_SCHEMA).expect("load mt536 schema");
        catalog.validate().expect("valid schema");
        let inbound = InboundMessage {
            id: "msg-536-two".into(),
            message_type: "MT536".into(),
            body: MT536_TWO_INSTRUMENTS_FIN.into(),
        };
        let parsed = parse_message(inbound.body.as_bytes());
        let batch = materialize_message(&catalog, &inbound, &parsed).expect("materialize");

        let events = SwiftSource.ingest(&batch);
        assert_eq!(events.len(), 2, "one event per TRADDET instrument");

        let eur = events
            .iter()
            .find(|e| e.isin.as_deref() == Some("IE00BD1RP616"))
            .expect("first instrument present");
        let gbp = events
            .iter()
            .find(|e| e.isin.as_deref() == Some("GB00B03MLX29"))
            .expect("second instrument present");

        assert_eq!(eur.amount, Some(125_000.0));
        assert_eq!(eur.currency.as_deref(), Some("EUR"));
        assert_eq!(gbp.amount, Some(54_321.0));
        assert_eq!(gbp.currency.as_deref(), Some("GBP"));

        assert_eq!(eur.safekeeping_account.as_deref(), Some("ACCT-EU-001"));
        assert_eq!(gbp.safekeeping_account.as_deref(), Some("ACCT-GB-002"));
        assert_eq!(eur.quantity, Some(2500.0));
        assert_eq!(gbp.quantity, Some(1000.0));
    }

    #[test]
    fn in_branch_matches_occurrence_correlated_cousins_only() {
        // Cousins: same occurrence, different top-level sequence name — the
        // MT536 SETDET[N]/AMT <-> TRADDET[N] shape.
        assert!(in_branch("SETDET[0]/AMT[0]", "TRADDET[0]"));
        assert!(in_branch("TRADDET[0]", "SETDET[0]/AMT[0]"));
        // A second instrument's settlement amount must never attribute to
        // the first instrument, and vice versa.
        assert!(!in_branch("SETDET[1]/AMT[0]", "TRADDET[0]"));
        assert!(!in_branch("SETDET[0]/AMT[0]", "TRADDET[1]"));
        // A second instrument's own direct (depth-1) fields must never
        // attribute to the first instrument either — the latent gap the old
        // "any two same-depth paths are siblings" rule left open.
        assert!(!in_branch("TRADDET[1]", "TRADDET[0]"));
        // True siblings under a shared parent occurrence (MT537 shape) are
        // unaffected by the occurrence-correlation rule for cousins.
        assert!(in_branch(
            "STAT[0]/TRAN[1]/LINK[0]",
            "STAT[0]/TRAN[1]/TRANSDET[0]"
        ));
        assert!(!in_branch(
            "STAT[0]/TRAN[0]/LINK[0]",
            "STAT[0]/TRAN[1]/TRANSDET[0]"
        ));
        // Ancestor/descendant, either direction, unaffected.
        assert!(in_branch("TRADDET[0]/FIAC[0]", "TRADDET[0]"));
        assert!(in_branch("TRADDET[0]", "TRADDET[0]/FIAC[0]"));
    }
}
