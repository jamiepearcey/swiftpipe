use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
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

fn put_manifest(tempdir: &tempfile::TempDir, job_id: &str, bytes: &[u8]) {
    let path = tempdir
        .path()
        .join("objects")
        .join("swiftpipe-outbox")
        .join("jobs")
        .join(job_id)
        .join("manifest.json");
    std::fs::create_dir_all(path.parent().expect("manifest parent")).expect("manifest parent");
    std::fs::write(path, bytes).expect("manifest write");
}

async fn get_manifest(router: axum::Router, job_id: &str) -> (StatusCode, Value) {
    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/jobs/{job_id}/manifest"))
                .body(Body::empty())
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

#[tokio::test]
async fn manifest_returns_stored_manifest() {
    let (tempdir, router) = test_router();
    put_manifest(
        &tempdir,
        "job-1",
        json!({
            "job_id": "job-1",
            "status": "completed",
            "counts": {"messages": 1}
        })
        .to_string()
        .as_bytes(),
    );

    let (status, body) = get_manifest(router, "job-1").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["job_id"], "job-1");
    assert_eq!(body["status"], "completed");
    assert_eq!(body["counts"]["messages"], 1);
}

#[tokio::test]
async fn manifest_returns_not_found_for_missing_manifest() {
    let (_tempdir, router) = test_router();

    let (status, body) = get_manifest(router, "missing-job").await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "not_found");
}

#[tokio::test]
async fn manifest_reports_internal_error_for_invalid_stored_json() {
    let (tempdir, router) = test_router();
    put_manifest(&tempdir, "bad-json", b"{not-json");

    let (status, body) = get_manifest(router, "bad-json").await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["code"], "internal_error");
}
