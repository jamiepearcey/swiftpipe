use axum::body::{to_bytes, Body};
use axum::http::{header, Method, Request, StatusCode};
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

async fn request(router: axum::Router, method: Method, uri: &str) -> (StatusCode, String, Vec<u8>) {
    let response = router
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .map_or("", |value| value.to_str().expect("content type"))
        .to_string();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    (status, content_type, bytes.to_vec())
}

#[tokio::test]
async fn openapi_returns_json_document() {
    let (_tempdir, router) = test_router();

    let (status, content_type, body) = request(router, Method::GET, "/openapi.json").await;
    let body: Value = serde_json::from_slice(&body).expect("openapi json");

    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type, "application/json");
    assert_eq!(body["openapi"], "3.1.0");
    assert!(body["paths"]["/v1/upload"].is_object());
}

#[tokio::test]
async fn openapi_rejects_unsupported_method() {
    let (_tempdir, router) = test_router();

    let (status, _content_type, _body) = request(router, Method::POST, "/openapi.json").await;

    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn openapi_adjacent_unknown_path_is_not_found() {
    let (_tempdir, router) = test_router();

    let (status, _content_type, _body) =
        request(router, Method::GET, "/openapi.json/missing").await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}
