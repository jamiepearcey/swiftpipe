//! `msg.field` derivation from a `swift_core::ParsedMessage` (contract section 2).
//! One row per block-4 field, generic over every MT type: the catalog only
//! contributes anchored-sequence frames; there is no per-type code.

use swift_core::{BlockId, FieldSlice, ParsedMessage, SequenceFrame};

use crate::identity::bic11_from_lt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldRow {
    pub field_seq: i64,
    pub seq_path: String,
    pub seq_index: String,
    pub tag: String,
    pub qualifier: Option<String>,
    pub issuer: Option<String>,
    pub value: String,
    pub occurrence: i64,
    pub raw: String,
}

fn s(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// `(seq_path, seq_index)`; both `"$"` at the root.
pub fn sequence_strings(path: &[SequenceFrame<'_>]) -> (String, String) {
    if path.is_empty() {
        return ("$".into(), "$".into());
    }
    let names: Vec<String> = path.iter().map(|f| s(f.name)).collect();
    let idx: Vec<String> = path
        .iter()
        .map(|f| format!("{}[{}]", s(f.name), f.occurrence))
        .collect();
    (names.join("/"), idx.join("/"))
}

/// `:QUAL/ISSR/value` (data source scheme) that swift-core leaves unqualified.
fn issuer_form(raw: &str) -> Option<(String, String, usize)> {
    let b = raw.as_bytes();
    if b.first() != Some(&b':') || b.len() < 7 {
        return None;
    }
    let alnum = |c: u8| c.is_ascii_uppercase() || c.is_ascii_digit();
    if !b[1..5].iter().all(|&c| alnum(c)) || b[5] != b'/' {
        return None;
    }
    let mut end = 6;
    while end < b.len() && end < 6 + 8 && alnum(b[end]) {
        end += 1;
    }
    if end == 6 || b.get(end) != Some(&b'/') {
        return None;
    }
    Some((raw[1..5].to_string(), raw[6..end].to_string(), end + 1))
}

pub fn derive_field(f: &FieldSlice<'_>) -> (Option<String>, Option<String>, String, String, String) {
    let raw = s(f.value);
    let mut qualifier = f.qualifier.map(s);
    let mut issuer = None;
    let mut skip = 0;
    if qualifier.is_some() {
        skip = 7; // ":QUAL//"
    } else if let Some((q, i, n)) = issuer_form(&raw) {
        qualifier = Some(q);
        issuer = Some(i);
        skip = n;
    }
    let value = raw.get(skip..).unwrap_or("").replace("\r\n", "\n");
    (qualifier, issuer, value, raw, s(f.tag))
}

pub fn derive_fields(parsed: &ParsedMessage<'_>) -> Vec<FieldRow> {
    let mut seen: std::collections::HashMap<(String, String, Option<String>), i64> =
        Default::default();
    let mut out = Vec::with_capacity(parsed.fields.len());
    for (n, f) in parsed.fields.iter().enumerate() {
        let (seq_path, seq_index) = sequence_strings(&f.sequence_path);
        let (qualifier, issuer, value, raw, tag) = derive_field(f);
        let c = seen
            .entry((seq_index.clone(), tag.clone(), qualifier.clone()))
            .or_insert(0);
        let occurrence = *c;
        *c += 1;
        out.push(FieldRow {
            field_seq: n as i64,
            seq_path,
            seq_index,
            tag,
            qualifier,
            issuer,
            value,
            occurrence,
            raw,
        });
    }
    out
}

/// Field-level completeness: block 4 (CRLF -> LF, trimmed, `-` terminator
/// dropped) equals the rows rendered `:tag:raw` joined by `\n`.
pub fn block4_matches_fields(parsed: &ParsedMessage<'_>, rows: &[FieldRow]) -> bool {
    let Some(b4) = parsed.text_block() else {
        return rows.is_empty();
    };
    let text = String::from_utf8_lossy(b4.content).replace("\r\n", "\n");
    let mut text = text.trim();
    if let Some(t) = text.strip_suffix('-') {
        text = t.trim_end();
    }
    let rendered: Vec<String> = rows
        .iter()
        .map(|r| format!(":{}:{}", r.tag, r.raw.replace("\r\n", "\n")))
        .collect();
    rendered.join("\n") == text
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Envelope {
    pub io: Option<String>,
    pub sender_bic: Option<String>,
    pub receiver_bic: Option<String>,
}

fn block<'a>(parsed: &ParsedMessage<'a>, id: BlockId<'a>) -> Option<String> {
    parsed
        .blocks
        .iter()
        .find(|b| b.id == id)
        .map(|b| s(b.content))
}

/// Sender, receiver and I/O from blocks 1 and 2 (contract section 3).
pub fn envelope(parsed: &ParsedMessage<'_>) -> Envelope {
    let b1 = block(parsed, BlockId::BasicHeader).unwrap_or_default();
    let b2 = block(parsed, BlockId::ApplicationHeader).unwrap_or_default();
    let io = b2.get(..1).filter(|c| *c == "I" || *c == "O").map(String::from);
    let b1_lt = b1.get(3..15).and_then(bic11_from_lt);
    let (sender, receiver) = match io.as_deref() {
        Some("I") => (b1_lt, b2.get(4..16).and_then(bic11_from_lt)),
        Some("O") => (b2.get(14..26).and_then(bic11_from_lt), b1_lt),
        _ => (None, None),
    };
    Envelope {
        io,
        sender_bic: sender,
        receiver_bic: receiver,
    }
}

/// `:20:` (MT940/942) else `:20C::SEME`.
pub fn sender_ref(rows: &[FieldRow]) -> Option<String> {
    rows.iter()
        .find(|r| r.tag == "20")
        .or_else(|| {
            rows.iter()
                .find(|r| r.tag == "20C" && r.qualifier.as_deref() == Some("SEME"))
        })
        .map(|r| r.value.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MT540: &[u8] = b"{1:F01BANKBEBBAXXX0000000000}{2:I540BANKDEFFXXXXN}{4:\r\n:16R:GENL\r\n:20C::SEME//ABC123\r\n:22F::SETR/GEBR/TRAD\r\n:16R:LINK\r\n:20C::RELA//REL1\r\n:16S:LINK\r\n:16R:LINK\r\n:20C::RELA//REL2\r\n:16S:LINK\r\n:16S:GENL\r\n:70E::SPRO//line one\r\nline two\r\n-}";

    #[test]
    fn rows_positions_and_qualifiers() {
        let p = swift_core::parse_message(MT540);
        let rows = derive_fields(&p);
        assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
        let seme = &rows[1];
        assert_eq!(
            (seme.tag.as_str(), seme.qualifier.as_deref(), seme.value.as_str()),
            ("20C", Some("SEME"), "ABC123")
        );
        assert_eq!((seme.seq_path.as_str(), seme.seq_index.as_str()), ("GENL", "GENL[0]"));
        assert_eq!(seme.raw, ":SEME//ABC123");
        // issuer form that swift-core leaves unqualified
        let setr = &rows[2];
        assert_eq!(setr.qualifier.as_deref(), Some("SETR"));
        assert_eq!(setr.issuer.as_deref(), Some("GEBR"));
        assert_eq!(setr.value, "TRAD");
        // repeated nested sequence: a new [n]; occurrence restarts per instance
        let rela: Vec<_> = rows.iter().filter(|r| r.qualifier.as_deref() == Some("RELA")).collect();
        assert_eq!(rela[0].seq_index, "GENL[0]/LINK[0]");
        assert_eq!(rela[1].seq_index, "GENL[0]/LINK[1]");
        assert_eq!((rela[0].occurrence, rela[1].occurrence), (0, 0));
        assert_eq!(rela[0].seq_path, "GENL/LINK");
        // multi-line value, CRLF -> LF
        let spro = rows.iter().find(|r| r.tag == "70E").unwrap();
        assert_eq!(spro.value, "line one\nline two");
        assert_eq!(spro.seq_index, "$");
        // wire order and 16R/16S kept
        assert_eq!(rows[0].tag, "16R");
        assert_eq!(rows.iter().map(|r| r.field_seq).collect::<Vec<_>>(), (0..rows.len() as i64).collect::<Vec<_>>());
        assert!(block4_matches_fields(&p, &rows));
    }

    #[test]
    fn same_tag_in_one_sequence_counts_occurrence() {
        let m = b"{1:F01BANKBEBBAXXX0000000000}{2:I540BANKDEFFXXXXN}{4:\n:16R:A\n:20C::RELA//1\n:20C::RELA//2\n:16S:A\n-}";
        let rows = derive_fields(&swift_core::parse_message(m));
        let occ: Vec<i64> = rows.iter().filter(|r| r.tag == "20C").map(|r| r.occurrence).collect();
        assert_eq!(occ, vec![0, 1]);
    }

    #[test]
    fn envelope_in_and_out() {
        let p = swift_core::parse_message(MT540);
        let e = envelope(&p);
        assert_eq!(e.io.as_deref(), Some("I"));
        assert_eq!(e.sender_bic.as_deref(), Some("BANKBEBBXXX"));
        assert_eq!(e.receiver_bic.as_deref(), Some("BANKDEFFXXX"));
        let o = b"{1:F01BANKBEBBAXXX0000000000}{2:O5401200050101BANKGB22AXXX00000000000501011200N}{4:\n:20:X\n-}";
        let e = envelope(&swift_core::parse_message(o));
        assert_eq!(e.io.as_deref(), Some("O"));
        assert_eq!(e.sender_bic.as_deref(), Some("BANKGB22XXX"));
        assert_eq!(e.receiver_bic.as_deref(), Some("BANKBEBBXXX"));
    }
}
