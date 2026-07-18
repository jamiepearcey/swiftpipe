//! ISO 20022 **camt.053** (bank-to-customer statement) source adapter — the MX
//! twin of `swift-mt940`. It targets the *same* [`ingest_core::CashStatement`],
//! proving the cash model is format-agnostic across MT (SWIFT FIN) and MX
//! (ISO 20022 XML): "SWIFT is only one way to ingest this data" holds for cash
//! too.
//!
//! Read-only XML via `roxmltree`; tags are matched by **local name** so it works
//! across camt.053 versions/namespaces. Handles `Stmt` (one → one statement),
//! `Acct` (IBAN + Ccy), `Bal` (OPBD/CLBD/CLAV), and `Ntry` (amount, CdtDbtInd,
//! reversal, value/booking dates, BkTxCd, refs, AddtlTxInf).

use ingest_core::{Balance, CashEntry, CashSource, CashStatement, Direction};
use roxmltree::{Document, Node};

/// The camt.053 cash source adapter.
pub struct Camt053Source;

impl CashSource for Camt053Source {
    type Input<'a> = &'a str;
    fn source_id(&self) -> &'static str {
        "mx"
    }
    fn ingest(&self, xml: Self::Input<'_>) -> Vec<CashStatement> {
        parse_camt053(xml, "")
    }
}

/// Parse a camt.053 document into normalized cash statements (one per `<Stmt>`).
pub fn parse_camt053(xml: &str, message_id_prefix: &str) -> Vec<CashStatement> {
    let doc = match Document::parse(xml) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    doc.descendants()
        .filter(|n| is(*n, "Stmt"))
        .enumerate()
        .map(|(i, stmt)| parse_stmt(stmt, message_id_prefix, i))
        .collect()
}

fn parse_stmt(stmt: Node, prefix: &str, idx: usize) -> CashStatement {
    let acct = find(stmt, "Acct");
    let account = acct
        .and_then(|a| text(a, "IBAN"))
        .or_else(|| acct.and_then(|a| text(a, "Othr").and(text(a, "Id"))));
    let currency = acct.and_then(|a| text(a, "Ccy"));

    let mut opening = None;
    let mut closing = None;
    let mut available = None;
    for bal in stmt.descendants().filter(|n| is(*n, "Bal")) {
        let (code, balance) = parse_balance(bal);
        match code.as_str() {
            "OPBD" | "PRCD" => opening = Some(balance),
            "CLBD" => closing = Some(balance),
            "CLAV" | "FWAV" => available = Some(balance),
            _ => {}
        }
    }

    let entries = stmt
        .descendants()
        .filter(|n| is(*n, "Ntry"))
        .map(parse_entry)
        .collect();

    let stmt_id = text(stmt, "Id").unwrap_or_else(|| format!("{prefix}stmt-{}", idx + 1));

    CashStatement {
        source: "mx".to_string(),
        message_id: stmt_id,
        message_type: "camt.053".to_string(),
        account,
        statement_number: text(stmt, "ElctrncSeqNb").or_else(|| text(stmt, "LglSeqNb")),
        currency,
        opening_balance: opening,
        closing_balance: closing,
        available_balance: available,
        entries,
    }
}

fn parse_balance(bal: Node) -> (String, Balance) {
    let code = text(bal, "Cd").unwrap_or_default();
    let amt = find(bal, "Amt");
    let amount = amt.and_then(|n| n.text()).and_then(|t| t.trim().parse::<f64>().ok()).unwrap_or(0.0);
    let currency = amt.and_then(|n| n.attribute("Ccy")).map(str::to_string);
    let direction = match text(bal, "CdtDbtInd").as_deref() {
        Some("DBIT") => Direction::Debit,
        _ => Direction::Credit,
    };
    let date = find(bal, "Dt").and_then(|d| text(d, "Dt")).or_else(|| text(bal, "Dt"));
    (code, Balance { direction, date, currency, amount })
}

fn parse_entry(ntry: Node) -> CashEntry {
    let amt = find(ntry, "Amt");
    let amount = amt.and_then(|n| n.text()).and_then(|t| t.trim().parse::<f64>().ok()).unwrap_or(0.0);
    let base = match text(ntry, "CdtDbtInd").as_deref() {
        Some("DBIT") => Direction::Debit,
        _ => Direction::Credit,
    };
    let reversal = text(ntry, "RvslInd").as_deref() == Some("true");
    let direction = match (base, reversal) {
        (Direction::Credit, true) => Direction::ReversalCredit,
        (Direction::Debit, true) => Direction::ReversalDebit,
        (d, _) => d,
    };
    let value_date = find(ntry, "ValDt").and_then(|v| text(v, "Dt"));
    let entry_date = find(ntry, "BookgDt").and_then(|v| text(v, "Dt"));
    let transaction_type = find(ntry, "BkTxCd").and_then(|b| text(b, "Cd"));
    let customer_ref = text(ntry, "EndToEndId").or_else(|| text(ntry, "NtryRef"));
    let bank_ref = text(ntry, "AcctSvcrRef");
    let info = text(ntry, "AddtlTxInf");

    CashEntry {
        value_date,
        entry_date,
        direction,
        amount,
        signed_amount: direction.sign() * amount,
        transaction_type,
        customer_ref,
        bank_ref,
        info,
    }
}

fn is(n: Node, name: &str) -> bool {
    n.tag_name().name() == name
}

/// First descendant (self-inclusive) with the given local name.
fn find<'a, 'i>(node: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    node.descendants().find(|n| is(*n, name))
}

/// First descendant with the given local name that has non-empty text (handles
/// the nested `<Dt><Dt>…</Dt></Dt>` idiom by skipping the empty outer element).
fn text(node: Node, name: &str) -> Option<String> {
    node.descendants().filter(|n| is(*n, name)).find_map(|n| {
        let t = n.text().unwrap_or("").trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAMT: &str = include_str!("../../../examples/camt053_sample.xml");

    #[test]
    fn parses_camt053_to_same_cash_model_as_mt940() {
        let stmts = parse_camt053(CAMT, "");
        assert_eq!(stmts.len(), 1);
        let s = &stmts[0];
        assert_eq!(s.source, "mx");
        assert_eq!(s.message_type, "camt.053");
        assert_eq!(s.account.as_deref(), Some("GB29BANK60161331926819"));
        assert_eq!(s.currency.as_deref(), Some("EUR"));

        assert_eq!(s.opening_balance.as_ref().unwrap().amount, 100000.0);
        assert_eq!(s.closing_balance.as_ref().unwrap().amount, 120000.0);

        assert_eq!(s.entries.len(), 2);
        assert_eq!(s.entries[0].direction, Direction::Credit);
        assert_eq!(s.entries[0].amount, 25000.0);
        assert_eq!(s.entries[0].transaction_type.as_deref(), Some("TRF"));
        assert_eq!(s.entries[0].bank_ref.as_deref(), Some("BANKREF-A"));
        assert_eq!(s.entries[0].signed_amount, 25000.0);
        assert_eq!(s.entries[1].direction, Direction::Debit);
        assert_eq!(s.entries[1].signed_amount, -5000.0);

        // Same recon identity as MT940 — the model is format-agnostic.
        assert_eq!(s.net_movement(), 20000.0);
        assert_eq!(s.reconciles(), Some(true));
    }

    #[test]
    fn source_via_trait() {
        let out = Camt053Source.ingest(CAMT);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].source, "mx");
    }
}
