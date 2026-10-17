//! Source ids, message ids and source positions (contract section 3).

use anyhow::{bail, Result};
use sha2::{Digest, Sha256};

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// An operator-given source: `custodian:<BIC11>` or `broker:<name>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceId {
    /// Canonical spelling written to `source_id` (an 8-char BIC gets `XXX`).
    pub id: String,
    pub kind: &'static str,
    pub name: String,
}

impl SourceId {
    pub fn parse(s: &str) -> Result<Self> {
        if let Some(bic) = s.strip_prefix("custodian:") {
            let ok = (bic.len() == 8 || bic.len() == 11)
                && bic
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit());
            if !ok {
                bail!("source `{s}`: a custodian id is custodian:<BIC8|BIC11>, upper case");
            }
            let name = if bic.len() == 8 {
                format!("{bic}XXX")
            } else {
                bic.to_string()
            };
            return Ok(Self {
                id: format!("custodian:{name}"),
                kind: "custodian",
                name,
            });
        }
        if let Some(name) = s.strip_prefix("broker:") {
            let ok = !name.is_empty()
                && name.len() <= 40
                && name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
            if !ok {
                bail!("source `{s}`: a broker id is broker:<[a-z0-9-]{{1,40}}>");
            }
            return Ok(Self {
                id: s.to_string(),
                kind: "broker",
                name: name.to_string(),
            });
        }
        bail!("source `{s}`: expected custodian:<BIC11> or broker:<name>")
    }

    /// One source-position namespace per (source, file).
    pub fn srcns(&self, file_sha256: &str) -> String {
        format!("{}.{}.{}", self.kind, self.name, &file_sha256[..32])
    }

    pub fn message_id(&self, file_sha256: &str, offset: usize) -> String {
        let mut h = Sha256::new();
        h.update(self.id.as_bytes());
        h.update([0x1f]);
        h.update(file_sha256.as_bytes());
        h.update([0x1f]);
        h.update(offset.to_string().as_bytes());
        let hex: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
        format!("m-{}", &hex[..32])
    }
}

/// BIC11 from a 12-char FIN logical-terminal address `AAAABBCCLDDD`
/// (the LT code `L` at index 8 is dropped).
pub fn bic11_from_lt(lt: &str) -> Option<String> {
    if lt.len() != 12 || !lt.is_ascii() {
        return None;
    }
    Some(format!("{}{}", &lt[..8], &lt[9..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_spellings() {
        assert_eq!(
            SourceId::parse("custodian:BANKBEBB").unwrap().id,
            "custodian:BANKBEBBXXX"
        );
        assert_eq!(SourceId::parse("broker:sample").unwrap().kind, "broker");
        assert!(SourceId::parse("custodian:bankbebb").is_err());
        assert!(SourceId::parse("broker:Sample").is_err());
        assert!(SourceId::parse("sample").is_err());
    }

    #[test]
    fn ids_are_stable_and_offset_sensitive() {
        let s = SourceId::parse("custodian:BANKBEBBXXX").unwrap();
        let sha = sha256_hex(b"abc");
        assert_eq!(s.message_id(&sha, 0), s.message_id(&sha, 0));
        assert_ne!(s.message_id(&sha, 0), s.message_id(&sha, 41));
        assert!(s.message_id(&sha, 0).starts_with("m-"));
        assert_eq!(s.message_id(&sha, 0).len(), 34);
        assert_eq!(
            s.srcns(&sha),
            format!("custodian.BANKBEBBXXX.{}", &sha[..32])
        );
    }

    #[test]
    fn lt_to_bic11() {
        assert_eq!(bic11_from_lt("BANKBEBBAXXX").as_deref(), Some("BANKBEBBXXX"));
        assert_eq!(bic11_from_lt("SHORT"), None);
    }
}
