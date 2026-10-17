//! The engine door: `POST /v1/{ns}/{table}` (Parquet) and, opt-in, `/v1/artifacts`.

use anyhow::{anyhow, bail, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Landed,
    /// 409 `already committed`: the batch was applied by an earlier run.
    AlreadyLanded,
}

pub trait Poster {
    /// `table` is `field`, `raw`, ... in namespace `msg`.
    fn post(&self, table: &str, parquet: Vec<u8>, position: &str) -> Result<Outcome>;
    fn post_artifact(&self, bytes: &[u8]) -> Result<String>;
}

pub struct HttpPoster {
    base: String,
    token: Option<String>,
    agent: ureq::Agent,
}

impl HttpPoster {
    pub fn new(base: &str, token: Option<String>) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .into();
        Self {
            base: base.trim_end_matches('/').to_string(),
            token,
            agent,
        }
    }

    fn auth<B>(&self, mut req: ureq::RequestBuilder<B>) -> ureq::RequestBuilder<B> {
        if let Some(t) = &self.token {
            req = req.header("Authorization", format!("Bearer {t}"));
        }
        req
    }
}

impl Poster for HttpPoster {
    fn post(&self, table: &str, parquet: Vec<u8>, position: &str) -> Result<Outcome> {
        let url = format!("{}/v1/msg/{table}?tag=true&schema=strict", self.base);
        let req = self
            .agent
            .post(&url)
            .header("Content-Type", "application/vnd.apache.parquet")
            .header("X-RAE-Source-Position", position);
        let mut resp = self
            .auth(req)
            .send(&parquet[..])
            .map_err(|e| anyhow!("POST {url}: {e}"))?;
        let status = resp.status().as_u16();
        let body = resp.body_mut().read_to_string().unwrap_or_default();
        classify(status, &body, &url)
    }

    fn post_artifact(&self, bytes: &[u8]) -> Result<String> {
        let url = format!("{}/v1/artifacts", self.base);
        let req = self
            .agent
            .post(&url)
            .header("Content-Type", "application/octet-stream");
        let mut resp = self
            .auth(req)
            .send(bytes)
            .map_err(|e| anyhow!("POST {url}: {e}"))?;
        let status = resp.status().as_u16();
        let body = resp.body_mut().read_to_string().unwrap_or_default();
        if !(200..300).contains(&status) {
            bail!("POST {url}: {status} {body}");
        }
        let v: serde_json::Value = serde_json::from_str(&body)?;
        ["ref", "artifact_ref", "id", "hash", "sha256"]
            .iter()
            .find_map(|k| v.get(*k).and_then(|x| x.as_str()))
            .map(String::from)
            .ok_or_else(|| anyhow!("artifact response has no ref: {body}"))
    }
}

pub fn classify(status: u16, body: &str, what: &str) -> Result<Outcome> {
    match status {
        200..=299 => Ok(Outcome::Landed),
        409 if body.contains("already committed") => Ok(Outcome::AlreadyLanded),
        _ => bail!("{what}: {status} {body}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_already_committed_409_is_a_noop() {
        assert_eq!(classify(201, "", "x").unwrap(), Outcome::Landed);
        assert_eq!(
            classify(409, "source position 3 was already committed at or before watermark 3", "x")
                .unwrap(),
            Outcome::AlreadyLanded
        );
        assert!(classify(409, "schema mismatch", "x").is_err());
        assert!(classify(422, "", "x").is_err());
    }
}
