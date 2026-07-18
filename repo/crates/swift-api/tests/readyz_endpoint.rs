use axum::body::{to_bytes, Body};
use axum::http::{Method, Request, StatusCode};
use serde_json::Value;
use swift_api::{make_sync_router_for_test, make_sync_router_with_postgres_ready_check_for_test};
use tower::ServiceExt;

fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn router_with_paths(
    schema_path: std::path::PathBuf,
    object_root: std::path::PathBuf,
    work_root: std::path::PathBuf,
) -> axum::Router {
    make_sync_router_for_test(schema_path, object_root, work_root, 100 * 1024 * 1024)
}

async fn get_readyz(router: axum::Router) -> (StatusCode, Value) {
    let response = router
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/readyz")
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
async fn readyz_returns_ok_when_dependencies_exist() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let object_root = tempdir.path().join("objects");
    let work_root = tempdir.path().join("work");
    std::fs::create_dir_all(&object_root).expect("object root");
    std::fs::create_dir_all(&work_root).expect("work root");
    let router = router_with_paths(
        workspace_root().join("examples/schemas"),
        object_root,
        work_root,
    );

    let (status, body) = get_readyz(router).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn readyz_returns_unavailable_when_schema_path_is_missing() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let object_root = tempdir.path().join("objects");
    let work_root = tempdir.path().join("work");
    std::fs::create_dir_all(&object_root).expect("object root");
    std::fs::create_dir_all(&work_root).expect("work root");
    let router = router_with_paths(
        tempdir.path().join("missing-schemas"),
        object_root,
        work_root,
    );

    let (status, body) = get_readyz(router).await;

    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["status"], "error");
    assert_eq!(body["error"], "schema path not found");
}

#[tokio::test]
async fn readyz_returns_unavailable_when_data_directories_are_missing() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let router = router_with_paths(
        workspace_root().join("examples/schemas"),
        tempdir.path().join("missing-objects"),
        tempdir.path().join("missing-work"),
    );

    let (status, body) = get_readyz(router).await;

    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["status"], "error");
    assert_eq!(body["error"], "data directories not found");
}

#[tokio::test]
async fn readyz_returns_unavailable_when_postgres_probe_fails() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let object_root = tempdir.path().join("objects");
    let work_root = tempdir.path().join("work");
    std::fs::create_dir_all(&object_root).expect("object root");
    std::fs::create_dir_all(&work_root).expect("work root");
    let router = make_sync_router_with_postgres_ready_check_for_test(
        workspace_root().join("examples/schemas"),
        object_root,
        work_root,
        100 * 1024 * 1024,
        "postgres://swiftpipe:swiftpipe@127.0.0.1:1/swiftpipe".to_string(),
    );

    let (status, body) = get_readyz(router).await;

    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["status"], "error");
    assert_eq!(body["check"], "postgres");
    assert!(!body["error"].as_str().expect("error string").is_empty());
}
