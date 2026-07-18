use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use swift_api::{make_sync_router_with_jobs_for_test, TestJobStatus};
use tower::ServiceExt;

fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn test_router(jobs: Vec<(String, TestJobStatus)>) -> (tempfile::TempDir, axum::Router) {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let object_root = tempdir.path().join("objects");
    let work_root = tempdir.path().join("work");
    std::fs::create_dir_all(&object_root).expect("object root");
    std::fs::create_dir_all(&work_root).expect("work root");
    let router = make_sync_router_with_jobs_for_test(
        workspace_root().join("examples/schemas"),
        object_root,
        work_root,
        100 * 1024 * 1024,
        jobs,
    );
    (tempdir, router)
}

async fn get_jobs(router: axum::Router, uri: &str) -> (StatusCode, Value) {
    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
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
async fn jobs_list_empty_store_returns_empty_array() {
    let (_tempdir, router) = test_router(Vec::new());

    let (status, body) = get_jobs(router, "/v1/jobs").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!([]));
}

#[tokio::test]
async fn jobs_list_returns_newest_jobs_first() {
    let (_tempdir, router) = test_router(vec![
        ("job-old".to_string(), TestJobStatus::Queued),
        (
            "job-new".to_string(),
            TestJobStatus::Failed {
                error: "failed during test".to_string(),
            },
        ),
    ]);

    let (status, body) = get_jobs(router, "/v1/jobs").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body[0]["job_id"], "job-new");
    assert_eq!(body[0]["status"], "failed");
    assert_eq!(body[0]["error"], "failed during test");
    assert_eq!(body[1]["job_id"], "job-old");
    assert_eq!(body[1]["status"], "queued");
}

#[tokio::test]
async fn jobs_list_respects_limit_query() {
    let (_tempdir, router) = test_router(vec![
        ("job-1".to_string(), TestJobStatus::Queued),
        ("job-2".to_string(), TestJobStatus::Queued),
        ("job-3".to_string(), TestJobStatus::Queued),
    ]);

    let (status, body) = get_jobs(router, "/v1/jobs?limit=2").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().expect("array").len(), 2);
    assert_eq!(body[0]["job_id"], "job-3");
    assert_eq!(body[1]["job_id"], "job-2");
}
