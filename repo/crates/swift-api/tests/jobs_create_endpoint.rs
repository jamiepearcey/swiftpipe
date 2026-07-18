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

async fn post_jobs(router: axum::Router, body: Body) -> (StatusCode, Value) {
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/jobs")
                .header("content-type", "application/json")
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

#[tokio::test]
async fn jobs_create_sync_success_returns_completed_manifest() {
    let (tempdir, router) = test_router(100 * 1024 * 1024);
    let input_uri = "s3://swiftpipe-inbox/jobs/input.fin";
    put_local_object(&tempdir, input_uri, &sample_fin());

    let (status, body) = post_jobs(
        router,
        Body::from(
            json!({
                "input_uri": input_uri,
                "message_type": "MT540",
                "outputs": ["rendered"]
            })
            .to_string(),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "completed");
    assert_eq!(body["counts"]["input_objects"], 1);
    assert_eq!(body["counts"]["messages"], 1);
    assert_eq!(body["counts"]["rendered"], 1);
}

#[tokio::test]
async fn jobs_create_rejects_malformed_json() {
    let (_tempdir, router) = test_router(100 * 1024 * 1024);

    let (status, body) = post_jobs(router, Body::from("{not-json")).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "bad_request");
}

#[tokio::test]
async fn jobs_create_rejects_missing_input_selection() {
    let (_tempdir, router) = test_router(100 * 1024 * 1024);

    let (status, body) = post_jobs(router, Body::from("{}")).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "bad_request");
}

#[tokio::test]
async fn jobs_create_rejects_non_s3_input_uri_schemes() {
    for input_uri in [
        "file:///tmp/input.fin",
        "http://example.com/input.fin",
        "ftp://example.com/input.fin",
    ] {
        let (_tempdir, router) = test_router(100 * 1024 * 1024);

        let (status, body) = post_jobs(
            router,
            Body::from(json!({ "input_uri": input_uri }).to_string()),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST, "input_uri={input_uri}");
        assert_eq!(body["code"], "bad_request");
        assert!(body["error"]
            .as_str()
            .expect("error string")
            .contains("expected s3://"));
    }
}

#[tokio::test]
async fn jobs_create_rejects_non_s3_input_prefix_schemes() {
    for input_prefix in [
        "file:///tmp/inbox/",
        "http://example.com/inbox/",
        "ftp://example.com/inbox/",
    ] {
        let (_tempdir, router) = test_router(100 * 1024 * 1024);

        let (status, body) = post_jobs(
            router,
            Body::from(json!({ "input_prefix": input_prefix }).to_string()),
        )
        .await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "input_prefix={input_prefix}"
        );
        assert_eq!(body["code"], "bad_request");
        assert!(body["error"]
            .as_str()
            .expect("error string")
            .contains("expected s3://"));
    }
}

#[tokio::test]
async fn jobs_create_rejects_unknown_request_key() {
    let (_tempdir, router) = test_router(100 * 1024 * 1024);

    let (status, body) = post_jobs(
        router,
        Body::from(
            json!({
                "input_prefix": "s3://swiftpipe-inbox/jobs/",
                "out_prefix": "s3://typo-outbox/"
            })
            .to_string(),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "bad_request");
    assert!(body["error"]
        .as_str()
        .expect("error string")
        .contains("unknown field `out_prefix`"));
}
