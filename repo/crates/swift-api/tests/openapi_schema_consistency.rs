use axum::body::{to_bytes, Body};
use axum::http::{Method, Request, StatusCode};
use std::collections::BTreeSet;
use swift_api::{job_request_field_names_for_test, make_sync_router_for_test};
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

async fn openapi_json(router: axum::Router) -> serde_json::Value {
    let response = router
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/openapi.json")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    serde_json::from_slice(&bytes).expect("openapi json")
}

#[tokio::test]
async fn openapi_job_request_fields_match_request_type() {
    let (_tempdir, router) = test_router();
    let openapi = openapi_json(router).await;
    let schema = &openapi["components"]["schemas"]["JobRequest"];
    let properties = schema["properties"]
        .as_object()
        .expect("JobRequest properties object");

    let documented: BTreeSet<&str> = properties.keys().map(String::as_str).collect();
    let expected: BTreeSet<&str> = job_request_field_names_for_test().iter().copied().collect();

    assert_eq!(documented, expected);
    assert_eq!(schema["additionalProperties"], false);
}
