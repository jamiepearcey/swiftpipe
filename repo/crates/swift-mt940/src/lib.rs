//! MT940 (SWIFT customer cash statement) source adapter — the cash-domain twin
//! of `swift-normalize`. MT940 is **bespoke** (like MT537): its `:61:` statement
//! line packs value-date / entry-date / D-C mark / amount / transaction-type /
//! references into a single field, which the generic swiftpipe settlement
//! template can't decompose — so this is a dedicated parser over swift-core's
//! structural fields, producing the source-agnostic [`ingest_core::CashStatement`].
//!
//! This is what "Tier-1 cash P&L" needs: opening/closing balances + signed cash
//! entries (coupons, dividends, fees, interest) that reconcile.
//!
//! Fields handled: `:20:` ref, `:25:` account, `:28C:` statement no,
//! `:60a:` opening balance, `:61:` statement line, `:86:` info (attached to the
//! preceding entry), `:62a:` closing balance, `:64:` available balance.

use ingest_core::{Balance, CashEntry, CashSource, CashStatement, Direction};
use swift_core::parse_message;

/// The MT940 cash source adapter.
pub struct Mt940Source;

impl CashSource for Mt940Source {
    type Input<'a> = &'a str;
    fn source_id(&self) -> &'static str {
        "swift"
    }
    fn ingest(&self, fin: Self::Input<'_>) -> Vec<CashStatement> {
        parse_mt940(fin, "").into_iter().collect()
    }
}

/// Parse one raw MT940 FIN message into a normalized [`CashStatement`].
pub fn parse_mt940(fin: &str, message_id: &str) -> Option<CashStatement> {
    let parsed = parse_message(fin.as_bytes());
    let mut stmt = CashStatement {
        source: "swift".to_string(),
        message_id: message_id.to_string(),
        message_type: "MT940".to_string(),
        ..Default::default()
    };
    let mut saw_any = false;
    let mut last_was_entry = false;

    for f in &parsed.fields {
        let tag = std::str::from_utf8(f.tag).unwrap_or("");
        let val = std::str::from_utf8(f.value).unwrap_or("").trim();
        match tag {
            "20" => {
                saw_any = true;
                if stmt.message_id.is_empty() {
                    stmt.message_id = val.to_string();
                }
                last_was_entry = false;
            }
            "25" => {
                saw_any = true;
                stmt.account = Some(val.to_string());
                last_was_entry = false;
            }
            "28C" | "28" => {
                stmt.statement_number = Some(val.to_string());
                last_was_entry = false;
            }
            t if t.starts_with("60") => {
                stmt.opening_balance = parse_balance(val);
                last_was_entry = false;
            }
            "61" => {
                if let Some(e) = parse_statement_line(val) {
                    stmt.entries.push(e);
                    saw_any = true;
                    last_was_entry = true;
                }
            }
            "86" => {
                if last_was_entry {
                    if let Some(e) = stmt.entries.last_mut() {
                        e.info = Some(val.to_string());
                    }
                }
                last_was_entry = false;
            }
            t if t.starts_with("62") => {
                stmt.closing_balance = parse_balance(val);
                last_was_entry = false;
            }
            "64" => {
                stmt.available_balance = parse_balance(val);
                last_was_entry = false;
            }
            _ => last_was_entry = false,
        }
    }

    stmt.currency = stmt
        .opening_balance
        .as_ref()
        .and_then(|b| b.currency.clone())
        .or_else(|| stmt.closing_balance.as_ref().and_then(|b| b.currency.clone()));

    if saw_any {
        Some(stmt)
    } else {
        None
    }
}

/// `<C|D><YYMMDD><CCC><amount>` — e.g. `C260513EUR1000,00`.
fn parse_balance(s: &str) -> Option<Balance> {
    let (dir, rest) = match s.chars().next()? {
        'C' => (Direction::Credit, &s[1..]),
        'D' => (Direction::Debit, &s[1..]),
        _ => return None,
    };
    if rest.len() < 6 + 3 {
        return None;
    }
    let date = yymmdd_to_iso(&rest[0..6]);
    let currency = rest[6..9].to_string();
    let amount = parse_amount(&rest[9..])?;
    Some(Balance { direction: dir, date, currency: Some(currency), amount })
}

/// Parse a `:61:` statement line (first line only; supplementary → `info`).
fn parse_statement_line(s: &str) -> Option<CashEntry> {
    let mut lines = s.splitn(2, '\n');
    let head = lines.next()?.trim();
    let info = lines.next().map(str::trim).filter(|x| !x.is_empty()).map(str::to_string);
    if head.len() < 7 {
        return None;
    }

    let value_date = yymmdd_to_iso(&head[0..6]);
    let mut pos = 6;

    // Optional entry date MMDD (4 digits) — only if followed by a D/C mark.
    if head.len() >= pos + 5
        && head[pos..pos + 4].bytes().all(|b| b.is_ascii_digit())
        && head.as_bytes().get(pos + 4).is_some_and(|b| matches!(b, b'C' | b'D' | b'R'))
    {
        // Skip the optional entry date (MMDD); value_date is the P&L-relevant one.
        pos += 4;
    }
    let entry_date = None;

    // D/C mark: RC / RD / C / D.
    let dir = if head[pos..].starts_with("RC") {
        pos += 2;
        Direction::ReversalCredit
    } else if head[pos..].starts_with("RD") {
        pos += 2;
        Direction::ReversalDebit
    } else if head[pos..].starts_with('C') {
        pos += 1;
        Direction::Credit
    } else if head[pos..].starts_with('D') {
        pos += 1;
        Direction::Debit
    } else {
        return None;
    };

    // Optional 1-char funds code before the amount (amount starts with a digit).
    if head.as_bytes().get(pos).is_some_and(|b| b.is_ascii_alphabetic()) {
        pos += 1;
    }

    let rest = &head[pos..];
    let n_idx = rest.find('N')?;
    let amount = parse_amount(&rest[..n_idx])?;
    let after_n = &rest[n_idx + 1..];
    let (transaction_type, refs) = if after_n.len() >= 3 {
        (Some(after_n[..3].to_string()), &after_n[3..])
    } else {
        (None, "")
    };
    let (customer_ref, bank_ref) = match refs.split_once("//") {
        Some((c, b)) => (nonempty(c), nonempty(b)),
        None => (nonempty(refs), None),
    };

    Some(CashEntry {
        value_date,
        entry_date,
        direction: dir,
        amount,
        signed_amount: dir.sign() * amount,
        transaction_type,
        customer_ref,
        bank_ref,
        info,
    })
}

fn yymmdd_to_iso(s: &str) -> Option<String> {
    if s.len() == 6 && s.bytes().all(|b| b.is_ascii_digit()) {
        Some(format!("20{}-{}-{}", &s[0..2], &s[2..4], &s[4..6]))
    } else {
        None
    }
}

fn parse_amount(s: &str) -> Option<f64> {
    let t = s.trim().replace(',', ".");
    if t.is_empty() {
        return None;
    }
    t.parse::<f64>().ok()
}

fn nonempty(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MT940_FIN: &str = include_str!("../../../examples/mt940_sample.fin");

    #[test]
    fn parses_mt940_cash_statement() {
        let stmt = parse_mt940(MT940_FIN, "msg-940").expect("parse mt940");
        assert_eq!(stmt.account.as_deref(), Some("GB29BANK60161331926819"));
        assert_eq!(stmt.statement_number.as_deref(), Some("135/1"));
        assert_eq!(stmt.currency.as_deref(), Some("EUR"));

        let ob = stmt.opening_balance.as_ref().unwrap();
        assert_eq!(ob.direction, Direction::Credit);
        assert_eq!(ob.amount, 100000.0);
        let cb = stmt.closing_balance.as_ref().unwrap();
        assert_eq!(cb.amount, 120000.0);

        assert_eq!(stmt.entries.len(), 2);
        assert_eq!(stmt.entries[0].direction, Direction::Credit);
        assert_eq!(stmt.entries[0].amount, 25000.0);
        assert_eq!(stmt.entries[0].transaction_type.as_deref(), Some("TRF"));
        assert_eq!(stmt.entries[0].customer_ref.as_deref(), Some("CUST-A"));
        assert_eq!(stmt.entries[0].bank_ref.as_deref(), Some("BANKREF-A"));
        assert_eq!(stmt.entries[0].signed_amount, 25000.0);
        assert_eq!(stmt.entries[0].info.as_deref(), Some("Coupon receipt ACME 5% 2028"));
        assert_eq!(stmt.entries[1].direction, Direction::Debit);
        assert_eq!(stmt.entries[1].signed_amount, -5000.0);

        // Tier-1 cash recon identity: opening + Σ movements == closing.
        assert_eq!(stmt.net_movement(), 20000.0);
        assert_eq!(stmt.reconciles(), Some(true));
    }

    #[test]
    fn mt940_source_via_trait() {
        let events = Mt940Source.ingest(MT940_FIN);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].message_type, "MT940");
        assert_eq!(events[0].source, "swift");
    }

    #[test]
    fn reversal_credit_is_negative() {
        let e = parse_statement_line("2605130513RC1000,00NRTIREV//X").unwrap();
        assert_eq!(e.direction, Direction::ReversalCredit);
        assert_eq!(e.amount, 1000.0);
        assert_eq!(e.signed_amount, -1000.0);
    }
}
