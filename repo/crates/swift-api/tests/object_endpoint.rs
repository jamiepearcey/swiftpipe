use axum::body::{to_bytes, Body};
use axum::http::{header, Request, StatusCode};
use serde_json::Value;
use swift_api::make_sync_router_for_test;
use tower::ServiceExt;

fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn test_router() -> (tempfile::TempDir, axum::Router) {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let object_root = tempdir.path().join("objects");
    let work_root = tempdir.path().join("work");
    std::fs::create_dir_all(&object_root).expect("object root");
    std::fs::create_dir_all(&work_root).expect("work root");
    let router = make_sync_router_for_test(
        workspace_root().join("examples/schemas"),
        object_root,
        work_root,
        100 * 1024 * 1024,
    );
    (tempdir, router)
}

fn put_local_object(tempdir: &tempfile::TempDir, uri: &str, bytes: &[u8]) {
    let path = uri
        .strip_prefix("s3://")
        .expect("test URI uses s3 scheme")
        .split('/')
        .fold(tempdir.path().join("objects"), |path, segment| {
            path.join(segment)
        });
    std::fs::create_dir_all(path.parent().expect("object parent")).expect("object parent");
    std::fs::write(path, bytes).expect("object write");
}

async fn get_object(router: axum::Router, uri: &str) -> (StatusCode, Option<String>, Vec<u8>) {
    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/object/{uri}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .map(|value| value.to_str().expect("content type").to_string());
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    (status, content_type, bytes.to_vec())
}

fn json_body(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).expect("json response")
}

#[tokio::test]
async fn object_returns_stored_object() {
    let (tempdir, router) = test_router();
    let uri = "s3://swiftpipe-outbox/jobs/job-1/manifest.json";
    put_local_object(&tempdir, uri, br#"{"status":"completed"}"#);

    let (status, content_type, body) = get_object(router, uri).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type.as_deref(), Some("application/json"));
    assert_eq!(body, br#"{"status":"completed"}"#);
}

#[tokio::test]
async fn object_rejects_unsupported_uri_scheme() {
    let (_tempdir, router) = test_router();

    let (status, _content_type, body) = get_object(router, "file://tmp/object.json").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json_body(&body)["code"], "bad_request");
}

#[tokio::test]
async fn object_returns_not_found_for_missing_object() {
    let (_tempdir, router) = test_router();

    let (status, _content_type, body) =
        get_object(router, "s3://swiftpipe-outbox/jobs/missing/manifest.json").await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json_body(&body)["code"], "not_found");
}
