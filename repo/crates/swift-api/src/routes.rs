use axum::{
    body::Bytes,
    error_handling::HandleErrorLayer,
    extract::{ConnectInfo, DefaultBodyLimit, Path, Query, State},
    http::{header, HeaderMap, HeaderValue, Method, Request, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    BoxError, Json, Router,
};
use governor::clock::Clock;
use prometheus::{Encoder, TextEncoder};
use serde_json::json;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tower::timeout::TimeoutLayer;
use tower::ServiceBuilder;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing::Instrument;

use crate::auth::AuthLayer;
use crate::error::ApiErrorCode;
use crate::job::{new_job_id, process_job_request, process_upload, JobError};
use crate::job_store::JobStatus;
use crate::manifest::JobRequest;
use crate::object_store::{LocalObjectStore, ObjectStore};
use crate::queue::JobTask;
use crate::state::AppState;
use crate::ui::INDEX_HTML;

pub(crate) fn make_router(state: Arc<AppState>, ui_dist: Option<PathBuf>) -> Router {
    let max_upload = state.max_upload_bytes;
    let request_timeout = state.request_timeout;
    let rate_limit_layer =
        middleware::from_fn_with_state(Arc::clone(&state), rate_limit_middleware);
    let upload_content_length_layer =
        middleware::from_fn_with_state(Arc::clone(&state), upload_content_length_middleware);
    let v1_routes = Router::new()
        .route(
            "/upload",
            post(upload_handler)
                .layer(upload_content_length_layer)
                .layer(rate_limit_layer.clone()),
        )
        .route(
            "/jobs",
            post(jobs_handler)
                .layer(rate_limit_layer)
                .get(jobs_list_handler),
        )
        .route("/jobs/:job_id", get(job_status_handler))
        .route("/jobs/:job_id/manifest", get(manifest_handler))
        .route("/object/*uri", get(object_handler))
        .layer(
            ServiceBuilder::new()
                .layer(HandleErrorLayer::new(handle_timeout_error))
                .layer(TimeoutLayer::new(request_timeout)),
        );

    let mut router = Router::new()
        // UI + health
        .route("/", get(index_handler))
        .route("/healthz", get(healthz_handler))
        .route("/readyz", get(readyz_handler))
        // Observability
        .route("/metrics", get(metrics_prometheus_handler))
        .route("/metrics/json", get(metrics_json_handler))
        // API docs
        .route("/openapi.json", get(openapi_handler))
        .route("/docs", get(swagger_handler))
        // v1 API
        .nest("/v1", v1_routes)
        .layer(DefaultBodyLimit::max(max_upload))
        .layer(AuthLayer::from_env())
        .layer(make_cors_layer())
        .layer(middleware::from_fn(correlation_id_middleware))
        .layer(middleware::from_fn_with_state(
            Arc::clone(&state),
            api_error_metrics_middleware,
        ))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    if let Some(dist) = ui_dist {
        let index = dist.join("index.html");
        router = router.nest_service(
            "/app",
            ServeDir::new(dist).not_found_service(ServeFile::new(index)),
        );
    }

    router
}

async fn handle_timeout_error(error: BoxError) -> Response {
    if error.is::<tower::timeout::error::Elapsed>() {
        ApiError::gateway_timeout("request timed out").into_response()
    } else {
        ApiError::internal(anyhow::anyhow!("middleware error: {error}")).into_response()
    }
}

// ---------------------------------------------------------------------------
// Write rate limiting
// ---------------------------------------------------------------------------

async fn rate_limit_middleware(
    State(state): State<Arc<AppState>>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let key = rate_limit_key(&request);
    match state.rate_limiter.check_key(&key) {
        Ok(()) => next.run(request).await,
        Err(not_until) => rate_limit_exhausted_response(
            not_until.wait_time_from(state.rate_limiter.clock().now()),
        ),
    }
}

async fn upload_content_length_middleware(
    State(state): State<Arc<AppState>>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    if let Some(content_length) = request
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
    {
        if content_length > state.max_upload_bytes {
            return upload_too_large_error(state.max_upload_bytes).into_response();
        }
    }

    next.run(request).await
}

fn rate_limit_key(request: &Request<axum::body::Body>) -> String {
    request
        .headers()
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map_or_else(
            || {
                request
                    .extensions()
                    .get::<ConnectInfo<SocketAddr>>()
                    .map_or_else(|| "unknown".to_string(), |peer| peer.0.ip().to_string())
            },
            str::to_string,
        )
}

fn rate_limit_exhausted_response(retry_after: std::time::Duration) -> Response {
    let retry_after_secs = retry_after.as_secs().max(1).to_string();
    let mut response = ApiError::too_many_requests("rate limit exceeded").into_response();
    response.headers_mut().insert(
        header::RETRY_AFTER,
        HeaderValue::from_str(&retry_after_secs).expect("numeric retry-after header"),
    );
    response
}

async fn api_error_metrics_middleware(
    State(state): State<Arc<AppState>>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let response = next.run(request).await;
    if let Some(code) = response.extensions().get::<ApiErrorCode>() {
        state.metrics.api_error(code.as_str());
    }
    response
}

// ---------------------------------------------------------------------------
// Correlation ID middleware
// ---------------------------------------------------------------------------

const MAX_REQUEST_ID_CHARS: usize = 128;

async fn correlation_id_middleware(mut request: Request<axum::body::Body>, next: Next) -> Response {
    let request_id = request_id_from_headers(request.headers());
    if let Ok(value) = HeaderValue::from_str(&request_id) {
        request.headers_mut().insert("x-request-id", value);
    }

    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let span = tracing::info_span!(
        "swiftpipe.http_request",
        request_id = %request_id,
        method = %method,
        path = %path,
    );
    let mut response = next.run(request).instrument(span).await;
    if let Ok(value) = HeaderValue::from_str(&request_id) {
        response.headers_mut().insert("x-request-id", value);
    }
    response
}

fn request_id_from_headers(headers: &HeaderMap) -> String {
    headers
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter(|value| value.chars().count() <= MAX_REQUEST_ID_CHARS)
        .map_or_else(new_job_id, str::to_string)
}

// ---------------------------------------------------------------------------
// CORS
// ---------------------------------------------------------------------------

fn make_cors_layer() -> CorsLayer {
    make_cors_layer_from_origins(std::env::var("SWIFTPIPE_CORS_ORIGINS").ok().as_deref())
}

fn make_cors_layer_from_origins(origins_env: Option<&str>) -> CorsLayer {
    match origins_env.map(str::trim).filter(|value| !value.is_empty()) {
        Some(val) => {
            let origins: Vec<HeaderValue> = val
                .split(',')
                .filter_map(|s| s.trim().parse().ok())
                .collect();
            CorsLayer::new()
                .allow_origin(origins)
                .allow_methods([Method::GET, Method::POST])
                .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
        }
        None => CorsLayer::new().allow_origin(Vec::<HeaderValue>::new()),
    }
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

struct ApiError {
    code: ApiErrorCode,
    message: String,
}

impl ApiError {
    fn bad_request(msg: impl Into<String>) -> Self {
        Self {
            code: ApiErrorCode::BadRequest,
            message: msg.into(),
        }
    }

    fn not_found(msg: impl Into<String>) -> Self {
        Self {
            code: ApiErrorCode::NotFound,
            message: msg.into(),
        }
    }

    fn payload_too_large(msg: impl Into<String>) -> Self {
        Self {
            code: ApiErrorCode::PayloadTooLarge,
            message: msg.into(),
        }
    }

    fn service_unavailable(msg: impl Into<String>) -> Self {
        Self {
            code: ApiErrorCode::ServiceUnavailable,
            message: msg.into(),
        }
    }

    fn gateway_timeout(msg: impl Into<String>) -> Self {
        Self {
            code: ApiErrorCode::Timeout,
            message: msg.into(),
        }
    }

    fn too_many_requests(msg: impl Into<String>) -> Self {
        Self {
            code: ApiErrorCode::RateLimited,
            message: msg.into(),
        }
    }

    fn internal(err: anyhow::Error) -> Self {
        let redacted_error = redact_error_log(&err.to_string());
        tracing::error!(error = %redacted_error, "internal server error");
        Self {
            code: ApiErrorCode::Internal,
            message: err.to_string(),
        }
    }

    fn job(err: JobError) -> Self {
        let code = err.api_code();
        if code == ApiErrorCode::Internal {
            let redacted_error = redact_error_log(&err.to_string());
            tracing::error!(error = %redacted_error, "internal job error");
        }
        Self {
            code,
            message: err.to_string(),
        }
    }
}

fn redact_error_log(value: &str) -> String {
    redact_account_numbers(&redact_bic_codes(value))
}

fn redact_bic_codes(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut token = String::new();
    for char in value.chars() {
        if char.is_ascii_alphanumeric() {
            token.push(char);
            continue;
        }
        push_redacted_bic(&mut output, &mut token);
        output.push(char);
    }
    push_redacted_bic(&mut output, &mut token);
    output
}

fn push_redacted_bic(output: &mut String, token: &mut String) {
    if token.is_empty() {
        return;
    }
    if is_bic_token(token) {
        output.push_str("[REDACTED_BIC]");
    } else {
        output.push_str(token);
    }
    token.clear();
}

fn is_bic_token(value: &str) -> bool {
    let bytes = value.as_bytes();
    matches!(bytes.len(), 8 | 11)
        && bytes[0..4].iter().all(u8::is_ascii_uppercase)
        && bytes[4..6].iter().all(u8::is_ascii_uppercase)
        && bytes[6..8].iter().all(u8::is_ascii_alphanumeric)
        && bytes[8..].iter().all(u8::is_ascii_alphanumeric)
}

fn redact_account_numbers(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut digits = String::new();
    for char in value.chars() {
        if char.is_ascii_digit() {
            digits.push(char);
            continue;
        }
        push_redacted_digits(&mut output, &mut digits);
        output.push(char);
    }
    push_redacted_digits(&mut output, &mut digits);
    output
}

fn push_redacted_digits(output: &mut String, digits: &mut String) {
    if digits.is_empty() {
        return;
    }
    if digits.len() >= 6 {
        output.push_str("[REDACTED_ACCOUNT]");
    } else {
        output.push_str(digits);
    }
    digits.clear();
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response = (
            self.code.status(),
            Json(json!({
                "status": "error",
                "code": self.code.as_str(),
                "error": self.message,
            })),
        )
            .into_response();
        response.extensions_mut().insert(self.code);
        response
    }
}

type ApiResult<T> = Result<T, ApiError>;

// ---------------------------------------------------------------------------
// Handlers — static pages
// ---------------------------------------------------------------------------

async fn index_handler() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn healthz_handler() -> impl IntoResponse {
    Json(json!({"status": "ok"}))
}

async fn readyz_handler(State(state): State<Arc<AppState>>) -> Response {
    if !state.schema_path.exists() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"status": "error", "error": "schema path not found"})),
        )
            .into_response();
    }
    if !state.object_root.exists() || !state.work_root.exists() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"status": "error", "error": "data directories not found"})),
        )
            .into_response();
    }
    if let Some(ready_check) = &state.ready_check {
        match tokio::time::timeout(std::time::Duration::from_secs(1), ready_check.check()).await {
            Ok(Ok(())) => {}
            Ok(Err(reason)) => {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({
                        "status": "error",
                        "error": reason,
                        "check": ready_check.name(),
                    })),
                )
                    .into_response();
            }
            Err(_) => {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({
                        "status": "error",
                        "error": "readiness check timed out",
                        "check": ready_check.name(),
                    })),
                )
                    .into_response();
            }
        }
    }
    Json(json!({"status": "ok"})).into_response()
}

// ---------------------------------------------------------------------------
// Observability
// ---------------------------------------------------------------------------

async fn metrics_prometheus_handler(State(state): State<Arc<AppState>>) -> Response {
    let encoder = TextEncoder::new();
    let mut body = Vec::new();
    if let Err(error) = encoder.encode(&state.metrics.registry.gather(), &mut body) {
        return ApiError::internal(error.into()).into_response();
    }

    axum::http::Response::builder()
        .status(StatusCode::OK)
        .header(
            axum::http::header::CONTENT_TYPE,
            "application/openmetrics-text; version=1.0.0",
        )
        .body(axum::body::Body::from(body))
        // Invariant: all response parts are static valid HTTP status/header values.
        .expect("static response must build")
}

async fn metrics_json_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let m = &state.metrics;
    Json(json!({
        "jobs_submitted": m.jobs_submitted.get(),
        "jobs_completed": m.jobs_completed.get(),
        "jobs_failed": m.jobs_failed.get(),
        "jobs_in_flight": m.jobs_in_flight.get().max(0),
        "queue_depth": m.queue_depth.get().max(0),
    }))
}

// ---------------------------------------------------------------------------
// API docs
// ---------------------------------------------------------------------------

const OPENAPI_JSON: &str = include_str!("openapi.json");

async fn openapi_handler() -> Response {
    axum::http::Response::builder()
        .status(StatusCode::OK)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(OPENAPI_JSON))
        // Invariant: all response parts are static valid HTTP status/header values.
        .expect("static response must build")
}

async fn swagger_handler() -> Html<&'static str> {
    Html(SWAGGER_HTML)
}

const SWAGGER_HTML: &str = r##"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>SwiftPipe API Docs</title>
  <link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css">
  <style>
    body { margin: 0; }
    .swagger-ui .topbar { background: #176b62; }
    .swagger-ui .topbar .download-url-wrapper { display: none; }
  </style>
</head>
<body>
  <div id="swagger-ui"></div>
  <script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
  <script>
    SwaggerUIBundle({
      url: "/openapi.json",
      dom_id: "#swagger-ui",
      presets: [SwaggerUIBundle.presets.apis, SwaggerUIBundle.SwaggerUIStandalonePreset],
      layout: "BaseLayout",
      deepLinking: true,
      tryItOutEnabled: true,
    });
  </script>
</body>
</html>"##;

// ---------------------------------------------------------------------------
// Job API handlers
// ---------------------------------------------------------------------------

async fn upload_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<impl IntoResponse> {
    state.metrics.upload_body_bytes(body.len());
    if body.len() > state.max_upload_bytes {
        return Err(upload_too_large_error(state.max_upload_bytes));
    }
    let message_type = params.get("message_type").cloned();
    let outputs = params.get("outputs").map(|v| {
        v.split(',')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>()
    });

    let idempotency_key = idempotency_key(&headers);
    if let Some(key) = idempotency_key.as_deref() {
        if let Some(job_id) = state.idempotency.get(key) {
            return Ok(queued_job_response(job_id));
        }
    }

    let job_id = new_job_id();

    if let Some(ref queue) = state.job_queue {
        state.job_store.insert(job_id.clone(), JobStatus::Queued);
        state.metrics.submitted();

        let task = JobTask::Upload {
            job_id: job_id.clone(),
            body: body.to_vec(),
            message_type,
            outputs,
        };
        queue.submit(task).map_err(|_| {
            state.metrics.failed(None);
            ApiError::service_unavailable("job queue is full; try again later")
        })?;
        if let Some(key) = idempotency_key {
            state.idempotency.insert(key, job_id.clone());
        }

        return Ok(queued_job_response(job_id));
    }

    let body_vec = body.to_vec();
    let result = tokio::task::spawn_blocking(move || {
        process_upload(&state, job_id, body_vec, message_type, outputs)
    })
    .await
    .map_err(|e| ApiError::internal(anyhow::anyhow!("task panicked: {e}")))?;

    let manifest = result.map_err(map_job_error)?;
    Ok(Json(json_value(&manifest)?).into_response())
}

async fn jobs_handler(
    State(state): State<Arc<AppState>>,
    body: Bytes,
) -> ApiResult<impl IntoResponse> {
    let mut request: JobRequest = serde_json::from_slice(&body).map_err(|err| {
        ApiError::bad_request(format!(
            "request body must be a valid JSON job request: {err}"
        ))
    })?;

    let job_id = new_job_id();
    request.output_prefix = Some(format!("s3://swiftpipe-outbox/jobs/{job_id}/"));

    if let Some(ref queue) = state.job_queue {
        state.job_store.insert(job_id.clone(), JobStatus::Queued);
        state.metrics.submitted();

        let task = JobTask::Batch {
            job_id: job_id.clone(),
            request,
        };
        queue.submit(task).map_err(|_| {
            state.metrics.failed(None);
            ApiError::service_unavailable("job queue is full; try again later")
        })?;

        return Ok((
            StatusCode::ACCEPTED,
            Json(json!({
                "job_id": job_id,
                "status": "queued",
                "poll_url": format!("/v1/jobs/{job_id}"),
            })),
        )
            .into_response());
    }

    let result = tokio::task::spawn_blocking(move || process_job_request(&state, job_id, request))
        .await
        .map_err(|e| ApiError::internal(anyhow::anyhow!("task panicked: {e}")))?;

    let manifest = result.map_err(map_job_error)?;
    Ok(Json(json_value(&manifest)?).into_response())
}

async fn jobs_list_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> ApiResult<impl IntoResponse> {
    let limit = params
        .get("limit")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(50)
        .min(500);
    let jobs = state.job_store.list_recent(limit);
    Ok(Json(json_value(&jobs)?))
}

async fn job_status_handler(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    match state.job_store.get(&job_id) {
        Some(view) => Ok(Json(json_value(&view)?).into_response()),
        None => Err(ApiError::not_found(format!("job {job_id} not found"))),
    }
}

async fn manifest_handler(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let store = LocalObjectStore::new(&state.object_root);
    let manifest_uri = format!("s3://swiftpipe-outbox/jobs/{job_id}/manifest.json");
    let bytes = store
        .get(&manifest_uri)
        .map_err(|_| ApiError::not_found(format!("manifest not found for job {job_id}")))?;
    state.metrics.object_store_op("get");
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| ApiError::internal(e.into()))?;
    Ok(Json(value))
}

async fn object_handler(
    State(state): State<Arc<AppState>>,
    Path(encoded_uri): Path<String>,
) -> ApiResult<Response> {
    let uri = encoded_uri;
    if !uri.starts_with("s3://") {
        return Err(ApiError::bad_request(format!(
            "unsupported object URI scheme for {uri}; expected s3://"
        )));
    }
    let store = LocalObjectStore::new(&state.object_root);
    let bytes = store.get(&uri).map_err(|e| {
        let msg = e.to_string();
        if msg.contains("unsupported object URI") || msg.contains("invalid object URI") {
            ApiError::bad_request(msg)
        } else {
            ApiError::not_found(format!("object not found: {uri}"))
        }
    })?;
    state.metrics.object_store_op("get");

    let content_type = content_type_for_uri(&uri);
    Ok(axum::http::Response::builder()
        .status(StatusCode::OK)
        .header(axum::http::header::CONTENT_TYPE, content_type)
        .body(axum::body::Body::from(bytes))
        // Invariant: status is static and content_type is selected from static constants.
        .expect("static response must build"))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn map_job_error(err: JobError) -> ApiError {
    ApiError::job(err)
}

fn upload_too_large_error(max_upload_bytes: usize) -> ApiError {
    ApiError::payload_too_large(format!(
        "request body exceeds configured limit of {max_upload_bytes} bytes"
    ))
}

fn json_value<T: serde::Serialize>(value: &T) -> ApiResult<serde_json::Value> {
    serde_json::to_value(value).map_err(|err| ApiError::internal(err.into()))
}

fn idempotency_key(headers: &HeaderMap) -> Option<String> {
    headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn queued_job_response(job_id: String) -> Response {
    (
        StatusCode::ACCEPTED,
        Json(json!({
            "job_id": job_id,
            "status": "queued",
            "poll_url": format!("/v1/jobs/{job_id}"),
        })),
    )
        .into_response()
}

fn content_type_for_uri(uri: &str) -> &'static str {
    let extension = std::path::Path::new(uri).extension();
    if extension.is_some_and(|ext| ext.eq_ignore_ascii_case("html")) {
        "text/html; charset=utf-8"
    } else if extension.is_some_and(|ext| ext.eq_ignore_ascii_case("json")) {
        "application/json"
    } else if extension.is_some_and(|ext| ext.eq_ignore_ascii_case("ndjson")) {
        "application/x-ndjson"
    } else if extension.is_some_and(|ext| ext.eq_ignore_ascii_case("zip")) {
        "application/zip"
    } else if extension.is_some_and(|ext| ext.eq_ignore_ascii_case("parquet")) {
        "application/vnd.apache.parquet"
    } else if extension.is_some_and(|ext| ext.eq_ignore_ascii_case("fin")) {
        "application/vnd.swift.fin"
    } else {
        "application/octet-stream"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use std::collections::BTreeMap;
    use std::num::NonZeroU32;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tower::ServiceExt;
    use tracing::field::{Field, Visit};
    use tracing::{Id, Subscriber};
    use tracing_subscriber::layer::{Context, SubscriberExt};
    use tracing_subscriber::Layer;

    #[derive(Debug, Clone)]
    struct CapturedSpan {
        name: String,
        fields: BTreeMap<String, String>,
    }

    #[derive(Debug)]
    struct CaptureLayer {
        spans: Arc<Mutex<Vec<CapturedSpan>>>,
    }

    impl<S> Layer<S> for CaptureLayer
    where
        S: Subscriber,
    {
        fn on_new_span(
            &self,
            attrs: &tracing::span::Attributes<'_>,
            _id: &Id,
            _ctx: Context<'_, S>,
        ) {
            let mut visitor = FieldVisitor::default();
            attrs.record(&mut visitor);
            self.spans
                .lock()
                .expect("span capture poisoned")
                .push(CapturedSpan {
                    name: attrs.metadata().name().to_string(),
                    fields: visitor.fields,
                });
        }
    }

    #[derive(Debug, Default)]
    struct FieldVisitor {
        fields: BTreeMap<String, String>,
    }

    impl Visit for FieldVisitor {
        fn record_str(&mut self, field: &Field, value: &str) {
            self.fields
                .insert(field.name().to_string(), value.to_string());
        }

        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            self.fields
                .insert(field.name().to_string(), format!("{value:?}"));
        }
    }

    fn rate_limited_router() -> Router {
        let root = std::env::temp_dir().join(format!("swiftpipe-rate-limit-{}", new_job_id()));
        let object_root = root.join("objects");
        let work_root = root.join("work");
        std::fs::create_dir_all(&object_root).expect("object root");
        std::fs::create_dir_all(&work_root).expect("work root");
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("workspace root")
            .to_path_buf();
        let state = Arc::new(AppState {
            schema_path: workspace_root.join("examples/schemas"),
            schema_catalog: crate::state::empty_schema_catalog(),
            object_root,
            work_root,
            max_upload_bytes: 1024,
            max_prefix_fanout: 10_000,
            max_prefix_parallelism: crate::state::DEFAULT_MAX_PREFIX_PARALLELISM,
            zip_max_total_bytes: crate::state::DEFAULT_ZIP_MAX_TOTAL_BYTES,
            zip_max_entries: crate::state::DEFAULT_ZIP_MAX_ENTRIES,
            persist_raw_text: false,
            persist_raw_fields: false,
            request_timeout: Duration::from_secs(120),
            rate_limiter: Arc::new(crate::state::make_rate_limiter(
                NonZeroU32::new(1).expect("non-zero rps"),
                NonZeroU32::new(1).expect("non-zero burst"),
            )),
            idempotency: Arc::new(crate::state::default_idempotency_store()),
            system_of_record: crate::system_record::SystemOfRecordConfig::None,
            ready_check: None,
            job_store: crate::job_store::JobStore::new(),
            job_queue: None,
            metrics: Arc::new(crate::state::Metrics::default()),
        });
        make_router(state, None)
    }

    fn queued_upload_router() -> Router {
        let root = std::env::temp_dir().join(format!("swiftpipe-idempotency-{}", new_job_id()));
        let object_root = root.join("objects");
        let work_root = root.join("work");
        std::fs::create_dir_all(&object_root).expect("object root");
        std::fs::create_dir_all(&work_root).expect("work root");
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("workspace root")
            .to_path_buf();
        let job_store = crate::job_store::JobStore::new();
        let metrics = Arc::new(crate::state::Metrics::default());
        let (sender, receiver) = tokio::sync::mpsc::channel(10);
        let _receiver = Box::leak(Box::new(receiver));
        let queue = crate::queue::JobQueue::from_sender_for_test(sender, Arc::clone(&metrics));
        let state = Arc::new(AppState {
            schema_path: workspace_root.join("examples/schemas"),
            schema_catalog: crate::state::empty_schema_catalog(),
            object_root,
            work_root,
            max_upload_bytes: 1024,
            max_prefix_fanout: 10_000,
            max_prefix_parallelism: crate::state::DEFAULT_MAX_PREFIX_PARALLELISM,
            zip_max_total_bytes: crate::state::DEFAULT_ZIP_MAX_TOTAL_BYTES,
            zip_max_entries: crate::state::DEFAULT_ZIP_MAX_ENTRIES,
            persist_raw_text: false,
            persist_raw_fields: false,
            request_timeout: Duration::from_secs(120),
            rate_limiter: Arc::new(crate::state::make_rate_limiter(
                NonZeroU32::new(5).expect("non-zero rps"),
                NonZeroU32::new(20).expect("non-zero burst"),
            )),
            idempotency: Arc::new(crate::state::default_idempotency_store()),
            system_of_record: crate::system_record::SystemOfRecordConfig::None,
            ready_check: None,
            job_store,
            job_queue: Some(queue),
            metrics,
        });
        make_router(state, None)
    }

    fn prefix_fanout_router(max_prefix_fanout: usize) -> Router {
        let root = std::env::temp_dir().join(format!("swiftpipe-prefix-fanout-{}", new_job_id()));
        let object_root = root.join("objects");
        let work_root = root.join("work");
        let input_dir = object_root.join("swiftpipe-inbox").join("bulk");
        std::fs::create_dir_all(&input_dir).expect("input dir");
        std::fs::create_dir_all(&work_root).expect("work root");
        for index in 0..100 {
            std::fs::write(input_dir.join(format!("{index:03}.fin")), b"not parsed")
                .expect("input file");
        }
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("workspace root")
            .to_path_buf();
        let state = Arc::new(AppState {
            schema_path: workspace_root.join("examples/schemas"),
            schema_catalog: crate::state::empty_schema_catalog(),
            object_root,
            work_root,
            max_upload_bytes: 1024,
            max_prefix_fanout,
            max_prefix_parallelism: crate::state::DEFAULT_MAX_PREFIX_PARALLELISM,
            zip_max_total_bytes: crate::state::DEFAULT_ZIP_MAX_TOTAL_BYTES,
            zip_max_entries: crate::state::DEFAULT_ZIP_MAX_ENTRIES,
            persist_raw_text: false,
            persist_raw_fields: false,
            request_timeout: Duration::from_secs(120),
            rate_limiter: Arc::new(crate::state::make_rate_limiter(
                NonZeroU32::new(5).expect("non-zero rps"),
                NonZeroU32::new(20).expect("non-zero burst"),
            )),
            idempotency: Arc::new(crate::state::default_idempotency_store()),
            system_of_record: crate::system_record::SystemOfRecordConfig::None,
            ready_check: None,
            job_store: crate::job_store::JobStore::new(),
            job_queue: None,
            metrics: Arc::new(crate::state::Metrics::default()),
        });
        make_router(state, None)
    }

    async fn slow_handler() -> &'static str {
        tokio::time::sleep(Duration::from_millis(50)).await;
        "done"
    }

    #[test]
    fn account_number_redaction_masks_long_digit_runs_in_error_text() {
        let redacted =
            redact_account_numbers("failed for :97A::SAFE//123456789012 and retry code 12345");

        assert_eq!(
            redacted,
            "failed for :97A::SAFE//[REDACTED_ACCOUNT] and retry code 12345"
        );
        assert!(!redacted.contains("123456789012"));
    }

    #[test]
    fn bic_redaction_masks_bic_tokens_in_error_text() {
        let redacted = redact_error_log(
            "routing failure for BANKBEBBXXX and DEUTDEFF with account 123456789012",
        );

        assert_eq!(
            redacted,
            "routing failure for [REDACTED_BIC] and [REDACTED_BIC] with account [REDACTED_ACCOUNT]"
        );
        assert!(!redacted.contains("BANKBEBBXXX"));
        assert!(!redacted.contains("DEUTDEFF"));
        assert!(!redacted.contains("123456789012"));
    }

    #[test]
    fn correlation_id_middleware_adds_request_id_to_request_span() {
        let mut last_spans = Vec::new();
        for _attempt in 0..3 {
            let app = rate_limited_router();
            let spans = Arc::new(Mutex::new(Vec::new()));
            let subscriber = tracing_subscriber::registry().with(CaptureLayer {
                spans: Arc::clone(&spans),
            });
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");

            let response = tracing::subscriber::with_default(subscriber, || {
                runtime.block_on(async {
                    app.oneshot(
                        Request::builder()
                            .uri("/healthz")
                            .header("x-request-id", "req-test-1")
                            .body(Body::empty())
                            .expect("request"),
                    )
                    .await
                    .expect("response")
                })
            });

            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                response
                    .headers()
                    .get("x-request-id")
                    .and_then(|value| value.to_str().ok()),
                Some("req-test-1")
            );
            let spans = spans.lock().expect("span capture poisoned");
            if let Some(span) = spans
                .iter()
                .find(|span| span.name == "swiftpipe.http_request")
            {
                assert_eq!(
                    span.fields.get("request_id").map(String::as_str),
                    Some("req-test-1")
                );
                assert_eq!(span.fields.get("method").map(String::as_str), Some("GET"));
                assert_eq!(
                    span.fields.get("path").map(String::as_str),
                    Some("/healthz")
                );
                return;
            }
            last_spans = spans.clone();
        }

        panic!("request span emitted; captured spans: {last_spans:?}");
    }

    #[test]
    fn request_id_from_headers_accepts_client_id_at_length_limit() {
        let mut headers = HeaderMap::new();
        let request_id = "r".repeat(MAX_REQUEST_ID_CHARS);
        headers.insert(
            "x-request-id",
            HeaderValue::from_str(&request_id).expect("valid request id"),
        );

        assert_eq!(request_id_from_headers(&headers), request_id);
    }

    #[test]
    fn request_id_from_headers_generates_for_overlong_client_id() {
        let mut headers = HeaderMap::new();
        let request_id = "r".repeat(MAX_REQUEST_ID_CHARS + 1);
        headers.insert(
            "x-request-id",
            HeaderValue::from_str(&request_id).expect("valid request id"),
        );

        let chosen = request_id_from_headers(&headers);

        assert_ne!(chosen, request_id);
        assert_eq!(chosen.len(), 36);
    }

    #[tokio::test]
    async fn timeout_layer_returns_gateway_timeout_for_slow_handler() {
        let app = Router::new().route("/slow", get(slow_handler)).layer(
            ServiceBuilder::new()
                .layer(HandleErrorLayer::new(handle_timeout_error))
                .layer(TimeoutLayer::new(Duration::from_millis(1))),
        );

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/slow")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let body: serde_json::Value = serde_json::from_slice(&body).expect("json body");

        assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(body["code"], "gateway_timeout");
    }

    #[tokio::test]
    async fn cors_without_configured_origins_does_not_reflect_origin() {
        let app = Router::new()
            .route("/healthz", get(healthz_handler))
            .layer(make_cors_layer_from_origins(None));

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/healthz")
                    .header(header::ORIGIN, "https://foo.example")
                    .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        assert!(!response
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN));
    }

    #[tokio::test]
    async fn write_rate_limit_returns_too_many_requests_when_bucket_is_exhausted() {
        let app = rate_limited_router();
        let mut saw_too_many_requests = false;

        for _ in 0..50 {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(Method::POST)
                        .uri("/v1/jobs")
                        .header("x-forwarded-for", "203.0.113.10")
                        .body(Body::from("not-json"))
                        .expect("request"),
                )
                .await
                .expect("response");

            if response.status() == StatusCode::TOO_MANY_REQUESTS {
                assert!(response.headers().contains_key(header::RETRY_AFTER));
                let body = to_bytes(response.into_body(), usize::MAX)
                    .await
                    .expect("body");
                let body: serde_json::Value = serde_json::from_slice(&body).expect("json body");
                assert_eq!(body["code"], "rate_limited");
                saw_too_many_requests = true;
                break;
            }
        }

        assert!(
            saw_too_many_requests,
            "tight loop should exhaust the write rate-limit bucket"
        );
    }

    #[tokio::test]
    async fn upload_idempotency_key_replays_previously_issued_job_id() {
        let app = queued_upload_router();
        let mut job_ids = Vec::new();

        for _ in 0..2 {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(Method::POST)
                        .uri("/v1/upload")
                        .header("idempotency-key", "upload-retry-key")
                        .body(Body::from("{1:F01BANKBEBBAXXX0000000000}{4:\n-}"))
                        .expect("request"),
                )
                .await
                .expect("response");
            assert_eq!(response.status(), StatusCode::ACCEPTED);
            let body = to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("body");
            let body: serde_json::Value = serde_json::from_slice(&body).expect("json body");
            job_ids.push(body["job_id"].as_str().expect("job id").to_string());
        }

        assert_eq!(job_ids[0], job_ids[1]);
    }

    #[tokio::test]
    async fn metrics_report_real_queue_depth_for_pending_tasks() {
        let app = queued_upload_router();
        for _ in 0..2 {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(Method::POST)
                        .uri("/v1/upload")
                        .body(Body::from("{1:F01BANKBEBBAXXX0000000000}{4:\n-}"))
                        .expect("request"),
                )
                .await
                .expect("response");
            assert_eq!(response.status(), StatusCode::ACCEPTED);
        }

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/metrics/json")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let body: serde_json::Value = serde_json::from_slice(&body).expect("json body");
        assert!(
            body["queue_depth"].as_u64().expect("queue depth") > 0,
            "pending queued uploads should be reflected in queue_depth"
        );
    }

    #[tokio::test]
    async fn prefix_job_returns_bad_request_when_fanout_exceeds_cap() {
        let app = prefix_fanout_router(50);
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/jobs")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"input_prefix":"s3://swiftpipe-inbox/bulk/","include_suffix":".fin"}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let body: serde_json::Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(body["code"], "bad_request");
        assert!(body["error"]
            .as_str()
            .expect("error")
            .contains("max_prefix_fanout 50"));
    }
}
