use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use serde_json::Value;
use swift_api::make_full_queue_router_for_test;
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

fn test_router() -> (tempfile::TempDir, axum::Router) {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let object_root = tempdir.path().join("objects");
    let work_root = tempdir.path().join("work");
    std::fs::create_dir_all(&object_root).expect("object root");
    std::fs::create_dir_all(&work_root).expect("work root");
    let router = make_full_queue_router_for_test(
        workspace_root().join("examples/schemas"),
        object_root,
        work_root,
        100 * 1024 * 1024,
    );
    (tempdir, router)
}

async fn post_upload(router: axum::Router) -> (StatusCode, Value) {
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/upload?message_type=MT540&outputs=rendered")
                .body(Body::from(sample_fin()))
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
async fn queued_upload_returns_service_unavailable_when_queue_is_full() {
    let (_tempdir, router) = test_router();

    let (status, body) = post_upload(router).await;

    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["status"], "error");
    assert_eq!(body["code"], "service_unavailable");
    assert_eq!(body["error"], "job queue is full; try again later");
}
