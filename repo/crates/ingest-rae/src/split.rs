//! How a file becomes messages (contract section 2).

use anyhow::{anyhow, bail, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// FIN (enveloped or bare MT940/942 statements).
    Mt,
    Camt,
    Csv,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Fin,
    FinBare,
    Camt { index: usize },
    Csv { header: String },
}

impl Kind {
    pub fn adapter(&self) -> &'static str {
        match self {
            Kind::Fin => "swift",
            Kind::FinBare => "swift_bare",
            Kind::Camt { .. } => "mx",
            Kind::Csv { .. } => "tabular",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Msg {
    pub offset: usize,
    /// The original bytes of the message, never a synthetic envelope.
    pub raw_text: String,
    /// What the parsers read (the synthetic FIN for a bare statement).
    pub parse_text: String,
    pub message_type: String,
    pub kind: Kind,
}

#[derive(Debug, Default)]
pub struct Split {
    pub messages: Vec<Msg>,
    pub skipped_fragments: usize,
}

/// Format detection by content, in the contract's order of precedence.
pub fn detect_format(file_name: &str, bytes: &[u8]) -> Result<Format> {
    let text = std::str::from_utf8(bytes).map_err(|_| anyhow!("refused: not UTF-8"))?;
    let ext = file_name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    if !swift_core::split_messages(bytes).is_empty() || has_bare_20(text) {
        return Ok(Format::Mt);
    }
    if ext == "xml" || text.trim_start().starts_with("<?xml") || text.contains("camt.053") {
        return Ok(Format::Camt);
    }
    let first = text.lines().next().unwrap_or("").to_ascii_lowercase();
    if ext == "csv" || (first.contains("isin") && first.contains(',')) {
        return Ok(Format::Csv);
    }
    bail!("refused: unknown format")
}

fn has_bare_20(text: &str) -> bool {
    text.starts_with(":20:") || text.contains("\n:20:")
}

pub fn split_file(format: Format, bytes: &[u8]) -> Result<Split> {
    let text = std::str::from_utf8(bytes).map_err(|_| anyhow!("refused: not UTF-8"))?;
    match format {
        Format::Mt => split_mt(bytes, text),
        Format::Camt => split_camt(text),
        Format::Csv => split_csv(text),
    }
}

fn split_mt(bytes: &[u8], text: &str) -> Result<Split> {
    let frags = swift_core::split_messages(bytes);
    if !frags.is_empty() {
        let starts = text.matches("{1:").count();
        let mut messages = Vec::new();
        let mut skipped = starts.saturating_sub(frags.len());
        for f in frags {
            let offset = f.as_ptr() as usize - bytes.as_ptr() as usize;
            let Some(t) = swift_core::message_type_of(f) else {
                skipped += 1;
                continue;
            };
            let raw = std::str::from_utf8(f)?.to_string();
            messages.push(Msg {
                offset,
                parse_text: raw.clone(),
                raw_text: raw,
                message_type: format!("MT{t}"),
                kind: Kind::Fin,
            });
        }
        return Ok(Split {
            messages,
            skipped_fragments: skipped,
        });
    }
    // Bare statements: the k-th `:20:` line of the original bytes is segment k.
    let mut starts: Vec<usize> = Vec::new();
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        if line.starts_with(":20:") {
            starts.push(at);
        }
        at += line.len();
    }
    let synth = swift_core::split_bare_statements(bytes);
    let mut segs: Vec<(usize, &str)> = Vec::new();
    for (k, &st) in starts.iter().enumerate() {
        let end = starts.get(k + 1).copied().unwrap_or(text.len());
        let seg = &text[st..end];
        let has = |tag: &str| seg.lines().any(|l| l.starts_with(tag));
        if has(":25:") || has(":25P:") {
            segs.push((st, seg));
        }
    }
    if segs.len() != synth.len() {
        bail!(
            "bare statement split disagrees with swift-core ({} vs {})",
            segs.len(),
            synth.len()
        );
    }
    let skipped = starts.len() - segs.len();
    let messages = segs
        .into_iter()
        .zip(synth)
        .map(|((offset, seg), (ty, fin))| Msg {
            offset,
            raw_text: seg.to_string(),
            parse_text: fin,
            message_type: format!("MT{ty}"),
            kind: Kind::FinBare,
        })
        .collect();
    Ok(Split {
        messages,
        skipped_fragments: skipped,
    })
}

fn split_camt(text: &str) -> Result<Split> {
    let doc = roxmltree::Document::parse(text).map_err(|e| anyhow!("refused: bad XML: {e}"))?;
    let messages = doc
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "Stmt")
        .enumerate()
        .map(|(index, n)| {
            let r = n.range();
            let raw = text[r.clone()].to_string();
            Msg {
                offset: r.start,
                parse_text: raw.clone(),
                raw_text: raw,
                message_type: "camt.053".into(),
                kind: Kind::Camt { index },
            }
        })
        .collect();
    Ok(Split {
        messages,
        skipped_fragments: 0,
    })
}

fn split_csv(text: &str) -> Result<Split> {
    let mut lines = text.split_inclusive('\n');
    let mut at = 0;
    let mut header = None;
    // First non-blank line is the header (as ingest-tabular reads it).
    for l in lines.by_ref() {
        at += l.len();
        if !l.trim().is_empty() {
            header = Some(l.trim_end_matches(['\r', '\n']).to_string());
            break;
        }
    }
    let header = header.ok_or_else(|| anyhow!("refused: empty CSV"))?;
    let cells: Vec<String> = header.split(',').map(|c| c.trim().to_ascii_lowercase()).collect();
    if !(cells.iter().any(|c| c == "isin") && cells.iter().any(|c| c == "quantity")) {
        bail!("refused: unknown_layout (header `{header}` lacks isin and quantity)");
    }
    let mut messages = Vec::new();
    for l in lines {
        let offset = at;
        at += l.len();
        if l.trim().is_empty() {
            continue;
        }
        let raw = l.trim_end_matches(['\r', '\n']).to_string();
        messages.push(Msg {
            offset,
            parse_text: raw.clone(),
            raw_text: raw,
            message_type: "CSV_POSITIONS".into(),
            kind: Kind::Csv {
                header: header.clone(),
            },
        });
    }
    Ok(Split {
        messages,
        skipped_fragments: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fin_offsets_are_byte_positions() {
        let a = "{1:F01BANKBEBBAXXX0000000000}{2:I540BANKDEFFXXXXN}{4:\n:20:A\n-}";
        let b = "{1:F01BANKBEBBAXXX0000000000}{2:I541BANKDEFFXXXXN}{4:\n:20:B\n-}";
        let file = format!("{{1:F21BANKBEBBAXXX0000000000}}{{4:{{177:0}}{{451:0}}}}\n{a}\n$\n{b}\n");
        let s = split_file(Format::Mt, file.as_bytes()).unwrap();
        assert_eq!(s.messages.len(), 2);
        assert_eq!(s.skipped_fragments, 1);
        for m in &s.messages {
            assert_eq!(&file[m.offset..m.offset + m.raw_text.len()], m.raw_text);
        }
        assert_eq!(s.messages[0].message_type, "MT540");
        assert_eq!(s.messages[1].message_type, "MT541");
    }

    #[test]
    fn bare_statements_skip_without_account() {
        let f = "junk\n:20:ONE\n:25:ACC1\n:61:x\n-\n:20:TWO\n:28C:1\n:20:THREE\n:25:ACC3\n";
        let s = split_file(Format::Mt, f.as_bytes()).unwrap();
        assert_eq!(s.messages.len(), 2);
        assert_eq!(s.skipped_fragments, 1);
        assert_eq!(s.messages[0].offset, 5);
        assert!(s.messages[0].raw_text.starts_with(":20:ONE"));
        assert!(s.messages[0].parse_text.starts_with("{1:"));
        assert_eq!(&f[s.messages[1].offset..], s.messages[1].raw_text);
    }

    #[test]
    fn csv_rows_and_refused_layout() {
        let f = "isin,quantity\nUS0378331005,10\n\nGB00B03MLX29,5\n";
        let s = split_file(Format::Csv, f.as_bytes()).unwrap();
        assert_eq!(s.messages.len(), 2);
        assert_eq!(s.messages[0].offset, 14);
        assert_eq!(s.messages[1].raw_text, "GB00B03MLX29,5");
        assert_eq!(&f[s.messages[1].offset..s.messages[1].offset + 5], "GB00B");
        assert!(split_file(Format::Csv, b"csd,isin,penalty\nX,Y,1\n").is_err());
    }

    #[test]
    fn camt_statements() {
        let x = "<?xml version=\"1.0\"?><Document><BkToCstmrStmt><Stmt><Id>A</Id></Stmt><Stmt><Id>B</Id></Stmt></BkToCstmrStmt></Document>";
        let s = split_file(Format::Camt, x.as_bytes()).unwrap();
        assert_eq!(s.messages.len(), 2);
        assert_eq!(s.messages[1].raw_text, "<Stmt><Id>B</Id></Stmt>");
        assert_eq!(&x[s.messages[1].offset..][..6], "<Stmt>");
    }

    #[test]
    fn detect_orders_fin_first() {
        assert_eq!(detect_format("x.fin", b"{1:F01A}{2:I540B}{4:\n-}").unwrap(), Format::Mt);
        assert_eq!(detect_format("x", b"<?xml version=\"1.0\"?><a/>").unwrap(), Format::Camt);
        assert_eq!(detect_format("x.csv", b"a,b\n").unwrap(), Format::Csv);
        assert!(detect_format("x", b"hello").is_err());
        assert!(detect_format("x", &[0xff, 0xfe]).is_err());
    }
}
