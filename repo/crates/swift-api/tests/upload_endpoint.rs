use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use serde_json::Value;
use std::fs;
use swift_api::make_sync_router_for_test;
use tower::ServiceExt;

fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn sample_fin() -> Vec<u8> {
    std::fs::read(workspace_root().join("examples/mt540_sample.fin")).expect("sample FIN")
}

fn test_router(max_upload_bytes: usize) -> (tempfile::TempDir, axum::Router) {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let object_root = tempdir.path().join("objects");
    let work_root = tempdir.path().join("work");
    std::fs::create_dir_all(&object_root).expect("object root");
    std::fs::create_dir_all(&work_root).expect("work root");
    let router = make_sync_router_for_test(
        workspace_root().join("examples/schemas"),
        object_root,
        work_root,
        max_upload_bytes,
    );
    (tempdir, router)
}

fn copy_example_schemas(schema_dir: &std::path::Path) {
    fs::create_dir_all(schema_dir).expect("schema dir");
    for entry in fs::read_dir(workspace_root().join("examples/schemas")).expect("schema entries") {
        let entry = entry.expect("schema entry");
        let source = entry.path();
        if matches!(
            source.extension().and_then(|extension| extension.to_str()),
            Some("yaml" | "yml")
        ) {
            fs::copy(source, schema_dir.join(entry.file_name())).expect("copy schema");
        }
    }
}

async fn post_upload_raw(router: axum::Router, uri: &str, body: Vec<u8>) -> (StatusCode, Vec<u8>) {
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .body(Body::from(body))
                .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    (status, bytes.to_vec())
}

async fn post_upload_with_content_length(
    router: axum::Router,
    uri: &str,
    content_length: usize,
    body: Body,
) -> (StatusCode, Value) {
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-length", content_length.to_string())
                .body(body)
                .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    let body = serde_json::from_slice(&bytes).expect("json response");
    (status, body)
}

async fn post_upload_json(router: axum::Router, uri: &str, body: Vec<u8>) -> (StatusCode, Value) {
    let (status, bytes) = post_upload_raw(router, uri, body).await;
    let body = serde_json::from_slice(&bytes).expect("json response");
    (status, body)
}

#[tokio::test]
async fn upload_uses_cached_schema_catalog_after_schema_directory_is_removed() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let object_root = tempdir.path().join("objects");
    let work_root = tempdir.path().join("work");
    let schema_dir = tempdir.path().join("schemas");
    fs::create_dir_all(&object_root).expect("object root");
    fs::create_dir_all(&work_root).expect("work root");
    copy_example_schemas(&schema_dir);

    let router = make_sync_router_for_test(
        schema_dir.clone(),
        object_root,
        work_root,
        100 * 1024 * 1024,
    );
    fs::remove_dir_all(&schema_dir).expect("remove schema dir after startup");

    let (status, body) = post_upload_json(
        router,
        "/v1/upload?message_type=MT540&outputs=rendered",
        sample_fin(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "completed");
    assert_eq!(body["counts"]["messages"], 1);
}

#[tokio::test]
async fn upload_sync_success_returns_completed_manifest() {
    let (_tempdir, router) = test_router(100 * 1024 * 1024);

    let (status, body) = post_upload_json(
        router,
        "/v1/upload?message_type=MT540&outputs=rendered",
        sample_fin(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "completed");
    assert_eq!(body["counts"]["messages"], 1);
    assert_eq!(body["counts"]["rendered"], 1);
}

#[tokio::test]
async fn upload_rejects_unknown_output_selection() {
    let (_tempdir, router) = test_router(100 * 1024 * 1024);

    let (status, body) = post_upload_json(
        router,
        "/v1/upload?message_type=MT540&outputs=not-a-real-output",
        sample_fin(),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "bad_request");
}

#[tokio::test]
async fn upload_rejects_oversize_body() {
    let (_tempdir, router) = test_router(8);

    let (status, _body) =
        post_upload_raw(router, "/v1/upload?message_type=MT540", vec![b'X'; 9]).await;

    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn upload_rejects_oversize_content_length_before_reading_body() {
    let (_tempdir, router) = test_router(8);

    let (status, body) =
        post_upload_with_content_length(router, "/v1/upload?message_type=MT540", 9, Body::empty())
            .await;

    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body["status"], "error");
    assert_eq!(body["code"], "payload_too_large");
    assert_eq!(
        body["error"],
        "request body exceeds configured limit of 8 bytes"
    );
}
