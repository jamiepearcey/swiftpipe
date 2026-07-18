//! Tower middleware layer for bearer token authentication.
//!
//! # Overview
//!
//! [`AuthLayer`] wraps an inner [`tower::Service`] and gates every request
//! behind an `Authorization: Bearer <token>` check. `SWIFTPIPE_AUTH_TOKEN`
//! accepts a comma-separated list for token rotation; token values containing
//! commas are not supported. When no token is configured every request passes
//! through unchanged, making it trivial to disable auth in local / test
//! environments.
//!
//! ## Constant-time comparison
//!
//! Token comparison is done with [`constant_time_eq`] to prevent timing
//! side-channels that an attacker could exploit to enumerate valid tokens one
//! byte at a time.

use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use axum::{
    body::Body,
    http::{header, Request},
    response::Response,
};
use tower::{Layer, Service};

use crate::error::ApiErrorCode;

#[cfg(test)]
use axum::http::StatusCode;

pub(crate) const AUTH_TOKEN_ENV: &str = "SWIFTPIPE_AUTH_TOKEN";

// ---------------------------------------------------------------------------
// Constant-time byte comparison
// ---------------------------------------------------------------------------

/// Compare two byte slices in constant time.
///
/// Returns `true` iff `a` and `b` are identical.  The function always iterates
/// over every byte position (up to `min(a.len(), b.len())`) so that the
/// execution time does not vary with the position of the first differing byte.
/// A length mismatch returns `false` immediately — leaking only the length,
/// which is acceptable because token lengths are not secret.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// ---------------------------------------------------------------------------
// AuthLayer
// ---------------------------------------------------------------------------

/// A [`tower::Layer`] that enforces bearer-token authentication.
///
/// Construct via [`AuthLayer::from_env`] (reads `SWIFTPIPE_AUTH_TOKEN`) or
/// [`AuthLayer::new`] (explicit token, useful in tests).
#[derive(Clone, Debug)]
pub struct AuthLayer {
    required_tokens: Vec<String>,
}

impl AuthLayer {
    /// Create an [`AuthLayer`] by reading `SWIFTPIPE_AUTH_TOKEN` from the
    /// environment. Multiple tokens may be supplied as a comma-separated list.
    /// If the variable is absent or empty, auth is disabled (all requests pass
    /// through).
    pub fn from_env() -> Self {
        let token = std::env::var(AUTH_TOKEN_ENV)
            .ok()
            .filter(|value| !value.is_empty());
        Self::new(token)
    }

    /// Create an [`AuthLayer`] with an explicit token.
    ///
    /// Pass `None` to disable authentication entirely.
    pub fn new(token: Option<String>) -> Self {
        Self {
            required_tokens: parse_tokens(token.as_deref()),
        }
    }
}

fn parse_tokens(value: Option<&str>) -> Vec<String> {
    value
        .into_iter()
        .flat_map(|tokens| tokens.split(','))
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .collect()
}

pub(crate) fn has_configured_auth_token_from_env() -> bool {
    std::env::var(AUTH_TOKEN_ENV)
        .ok()
        .is_some_and(|value| !parse_tokens(Some(&value)).is_empty())
}

impl<S> Layer<S> for AuthLayer {
    type Service = AuthMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AuthMiddleware {
            inner,
            required_tokens: self.required_tokens.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// AuthMiddleware
// ---------------------------------------------------------------------------

/// The [`tower::Service`] produced by [`AuthLayer`].
#[derive(Clone, Debug)]
pub struct AuthMiddleware<S> {
    inner: S,
    required_tokens: Vec<String>,
}

/// Build the `401 Unauthorized` JSON response that is returned when the bearer
/// token is missing or incorrect.
fn unauthorized_response() -> Response {
    let code = ApiErrorCode::Unauthorized;
    let body = serde_json::json!({
        "status": "error",
        "code": code.as_str(),
        "error": "invalid or missing bearer token",
    });
    // Invariant: this JSON value contains only static string fields.
    let json_bytes = serde_json::to_vec(&body).expect("static JSON value must serialize");

    Response::builder()
        .status(code.status())
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json_bytes))
        // Invariant: status and content-type are static valid HTTP values.
        .expect("static response must build")
}

/// Returns `true` if `path` is an unauthenticated health-probe endpoint.
#[inline]
fn is_health_path(path: &str) -> bool {
    path.starts_with("/healthz") || path.starts_with("/readyz")
}

impl<S, B> Service<Request<B>> for AuthMiddleware<S>
where
    S: Service<Request<B>, Response = Response> + Send + Clone + 'static,
    S::Future: Send + 'static,
    B: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = AuthFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        // --- Auth disabled: forward unconditionally ---
        if self.required_tokens.is_empty() {
            return AuthFuture::Forwarded {
                future: self.inner.call(req),
            };
        }

        // --- Health probes bypass authentication ---
        if is_health_path(req.uri().path()) {
            return AuthFuture::Forwarded {
                future: self.inner.call(req),
            };
        }

        // --- Extract "Authorization: Bearer <token>" ---
        let provided = req
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "));

        // --- Constant-time comparison ---
        let authorized = match provided {
            Some(token) => self
                .required_tokens
                .iter()
                .any(|required| constant_time_eq(token.as_bytes(), required.as_bytes())),
            None => false,
        };

        if authorized {
            AuthFuture::Forwarded {
                future: self.inner.call(req),
            }
        } else {
            AuthFuture::Rejected {
                response: Some(unauthorized_response()),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// AuthFuture
// ---------------------------------------------------------------------------

// The future returned by AuthMiddleware::call.
// pin_project_lite is used to safely implement Future for the enum variant
// that wraps the inner service's future (which may need to be pinned).
pin_project_lite::pin_project! {
    #[project = AuthFutureProj]
    pub enum AuthFuture<F> {
        /// Request was forwarded to the inner service.
        Forwarded { #[pin] future: F },
        /// Request was rejected; the 401 response is returned immediately.
        Rejected { response: Option<Response> },
    }
}

impl<F, E> Future for AuthFuture<F>
where
    F: Future<Output = Result<Response, E>>,
{
    type Output = Result<Response, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            AuthFutureProj::Forwarded { future } => future.poll(cx),
            AuthFutureProj::Rejected { response } => Poll::Ready(Ok(response
                .take()
                // Invariant: futures must not be polled again after returning Ready.
                .expect("Rejected future polled more than once"))),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use axum::{body::Body, http::Request};
    use tower::util::BoxCloneService;
    use tower::{Layer, ServiceExt};

    /// Helper: build a no-op inner service that always returns 200 OK.
    fn ok_service() -> BoxCloneService<Request<Body>, Response, std::convert::Infallible> {
        BoxCloneService::new(tower::service_fn(|_req: Request<Body>| async {
            Ok::<_, std::convert::Infallible>(
                Response::builder()
                    .status(StatusCode::OK)
                    .body(Body::empty())
                    .unwrap(),
            )
        }))
    }

    /// Helper: build a GET request for the given path with an optional bearer token.
    fn request(path: &str, bearer: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder().method("GET").uri(path);
        if let Some(token) = bearer {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        builder.body(Body::empty()).unwrap()
    }

    // -----------------------------------------------------------------------
    // Test 1: Auth disabled (None token) — all requests pass through
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn no_token_all_requests_pass() {
        let layer = AuthLayer::new(None);
        let mut svc = layer.layer(ok_service());

        // No auth header → should still get 200
        let response = svc
            .ready()
            .await
            .unwrap()
            .call(request("/api/jobs", None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Wrong auth header → still 200 because auth is disabled
        let response = svc
            .ready()
            .await
            .unwrap()
            .call(request("/api/upload", Some("wrong-token")))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    // -----------------------------------------------------------------------
    // Test 2: Correct token → request is forwarded, returns 200
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn correct_token_passes() {
        let layer = AuthLayer::new(Some("secret-abc".to_string()));
        let mut svc = layer.layer(ok_service());

        let response = svc
            .ready()
            .await
            .unwrap()
            .call(request("/api/jobs", Some("secret-abc")))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn comma_separated_tokens_allows_rotation_tokens() {
        let layer = AuthLayer::new(Some("old-secret,new-secret".to_string()));
        let mut svc = layer.layer(ok_service());

        for token in ["old-secret", "new-secret"] {
            let response = svc
                .ready()
                .await
                .unwrap()
                .call(request("/api/jobs", Some(token)))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }

        let response = svc
            .ready()
            .await
            .unwrap()
            .call(request("/api/jobs", Some("unknown-secret")))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    // -----------------------------------------------------------------------
    // Test 3: Wrong token → 401 with correct JSON body
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn wrong_token_returns_401() {
        let layer = AuthLayer::new(Some("secret-abc".to_string()));
        let mut svc = layer.layer(ok_service());

        let response = svc
            .ready()
            .await
            .unwrap()
            .call(request("/api/jobs", Some("wrong-token")))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(body["status"], "error");
        assert_eq!(body["code"], "unauthorized");
        assert_eq!(body["error"], "invalid or missing bearer token");
    }

    // -----------------------------------------------------------------------
    // Test 4: Missing Authorization header → 401
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn missing_token_returns_401() {
        let layer = AuthLayer::new(Some("secret-abc".to_string()));
        let mut svc = layer.layer(ok_service());

        let response = svc
            .ready()
            .await
            .unwrap()
            .call(request("/api/jobs", None))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    // -----------------------------------------------------------------------
    // Test 5: /healthz and /readyz bypass auth even with a wrong token
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn health_probes_bypass_auth() {
        let layer = AuthLayer::new(Some("secret-abc".to_string()));
        let mut svc = layer.layer(ok_service());

        for path in ["/healthz", "/healthz/live", "/readyz", "/readyz/ready"] {
            let response = svc
                .ready()
                .await
                .unwrap()
                .call(request(path, None))
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "expected 200 for health path {path}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Test 6: constant_time_eq correctness
    // -----------------------------------------------------------------------

    #[test]
    fn constant_time_eq_identical() {
        assert!(constant_time_eq(b"hello", b"hello"));
    }

    #[test]
    fn constant_time_eq_different() {
        assert!(!constant_time_eq(b"hello", b"world"));
    }

    #[test]
    fn constant_time_eq_different_lengths() {
        assert!(!constant_time_eq(b"short", b"longer-string"));
    }

    #[test]
    fn constant_time_eq_empty() {
        assert!(constant_time_eq(b"", b""));
        assert!(!constant_time_eq(b"", b"x"));
    }

    // -----------------------------------------------------------------------
    // Test 7: from_env reads SWIFTPIPE_AUTH_TOKEN
    // -----------------------------------------------------------------------

    #[test]
    fn from_env_reads_env_var() {
        // Unset → auth disabled
        std::env::remove_var("SWIFTPIPE_AUTH_TOKEN");
        let layer = AuthLayer::from_env();
        assert!(layer.required_tokens.is_empty());

        // Set → auth enabled
        std::env::set_var("SWIFTPIPE_AUTH_TOKEN", "env-token");
        let layer = AuthLayer::from_env();
        assert_eq!(layer.required_tokens, ["env-token"]);

        // Empty string → treated as disabled
        std::env::set_var("SWIFTPIPE_AUTH_TOKEN", "");
        let layer = AuthLayer::from_env();
        assert!(layer.required_tokens.is_empty());

        // Comma-separated values → token rotation set
        std::env::set_var("SWIFTPIPE_AUTH_TOKEN", "old-token, new-token");
        let layer = AuthLayer::from_env();
        assert_eq!(layer.required_tokens, ["old-token", "new-token"]);

        // Clean up so other tests aren't affected
        std::env::remove_var("SWIFTPIPE_AUTH_TOKEN");
    }
}
