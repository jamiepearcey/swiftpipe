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

async fn get_job(router: axum::Router, job_id: &str) -> (StatusCode, Value) {
    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/jobs/{job_id}"))
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
async fn job_status_returns_queued_job() {
    let (_tempdir, router) = test_router(vec![("job-queued".to_string(), TestJobStatus::Queued)]);

    let (status, body) = get_job(router, "job-queued").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["job_id"], "job-queued");
    assert_eq!(body["status"], "queued");
}

#[tokio::test]
async fn job_status_returns_completed_job_manifest_summary() {
    let manifest = json!({
        "job_id": "job-completed",
        "status": "completed",
        "counts": {"messages": 1}
    });
    let (_tempdir, router) = test_router(vec![(
        "job-completed".to_string(),
        TestJobStatus::Completed { manifest },
    )]);

    let (status, body) = get_job(router, "job-completed").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["job_id"], "job-completed");
    assert_eq!(body["status"], "completed");
    assert_eq!(body["manifest"]["counts"]["messages"], 1);
}

#[tokio::test]
async fn job_status_returns_not_found_for_unknown_job() {
    let (_tempdir, router) = test_router(Vec::new());

    let (status, body) = get_job(router, "missing-job").await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "not_found");
}
