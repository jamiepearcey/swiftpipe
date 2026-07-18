use axum::body::{to_bytes, Body};
use axum::http::{header, Method, Request, StatusCode};
use serde_json::{json, Value};
use std::time::Duration;
use swift_api::{make_queued_router_for_test, make_sync_router_for_test};
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

fn prometheus_counter_value(body: &str, metric: &str, label: &str) -> u64 {
    let prefix = format!("{metric}{{{label}}} ");
    body.lines()
        .find_map(|line| {
            line.strip_prefix(&prefix)
                .and_then(|value| value.parse::<u64>().ok())
        })
        .unwrap_or_else(|| panic!("missing metric series {prefix}"))
}

fn prometheus_gauge_value(body: &str, metric: &str, label: &str) -> i64 {
    let prefix = format!("{metric}{{{label}}} ");
    body.lines()
        .find_map(|line| {
            line.strip_prefix(&prefix)
                .and_then(|value| value.parse::<i64>().ok())
        })
        .unwrap_or_else(|| panic!("missing metric series {prefix}"))
}

fn prometheus_unlabeled_value(body: &str, metric: &str) -> f64 {
    let prefix = format!("{metric} ");
    body.lines()
        .find_map(|line| {
            line.strip_prefix(&prefix)
                .and_then(|value| value.parse::<f64>().ok())
        })
        .unwrap_or_else(|| panic!("missing metric {metric}"))
}

#[tokio::test]
async fn metrics_prometheus_returns_text_counters() {
    let (_tempdir, router) = test_router();

    let (status, content_type, body) = request(router, Method::GET, "/metrics").await;
    let body = String::from_utf8(body).expect("utf8 metrics");

    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type, "application/openmetrics-text; version=1.0.0");
    assert!(body.contains("swiftpipe_jobs_submitted_total 0"));
    assert!(body.contains("swiftpipe_jobs_in_flight 0"));
    assert!(body.contains("swiftpipe_queue_depth 0"));
    assert!(body.contains("swiftpipe_job_duration_seconds_bucket"));
    assert!(body.contains("swiftpipe_job_duration_seconds_count 0"));
}

#[tokio::test]
async fn metrics_prometheus_reports_upload_body_bytes() {
    let (_tempdir, router) = test_router();
    let body = sample_fin();
    let expected_bytes = f64::from(u32::try_from(body.len()).expect("sample size fits u32"));

    let upload_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/upload?message_type=MT540&outputs=rendered")
                .body(Body::from(body))
                .expect("upload request"),
        )
        .await
        .expect("upload response");
    assert_eq!(upload_response.status(), StatusCode::OK);

    let (status, _content_type, body) = request(router, Method::GET, "/metrics").await;
    let body = String::from_utf8(body).expect("utf8 metrics");

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        prometheus_unlabeled_value(&body, "swiftpipe_upload_body_bytes_count"),
        1.0
    );
    assert_eq!(
        prometheus_unlabeled_value(&body, "swiftpipe_upload_body_bytes_sum"),
        expected_bytes
    );
}

#[tokio::test]
async fn metrics_prometheus_reports_jobs_by_status() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let object_root = tempdir.path().join("objects");
    let work_root = tempdir.path().join("work");
    std::fs::create_dir_all(&object_root).expect("object root");
    std::fs::create_dir_all(&work_root).expect("work root");
    let (router, _workers) = make_queued_router_for_test(
        workspace_root().join("examples/schemas"),
        object_root,
        work_root,
        100 * 1024 * 1024,
        1,
        8,
    );

    let upload_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/upload?message_type=MT540&outputs=rendered")
                .body(Body::from(sample_fin()))
                .expect("upload request"),
        )
        .await
        .expect("upload response");
    assert_eq!(upload_response.status(), StatusCode::ACCEPTED);

    let mut last_body = String::new();
    for _ in 0..20 {
        let (status, _content_type, body) = request(router.clone(), Method::GET, "/metrics").await;
        assert_eq!(status, StatusCode::OK);
        last_body = String::from_utf8(body).expect("utf8 metrics");
        if prometheus_gauge_value(&last_body, "swiftpipe_jobs_total", r#"status="succeeded""#) == 1
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    assert_eq!(
        prometheus_gauge_value(&last_body, "swiftpipe_jobs_total", r#"status="queued""#,),
        0
    );
    assert_eq!(
        prometheus_gauge_value(&last_body, "swiftpipe_jobs_total", r#"status="running""#,),
        0
    );
    assert_eq!(
        prometheus_gauge_value(&last_body, "swiftpipe_jobs_total", r#"status="succeeded""#,),
        1
    );
    assert_eq!(
        prometheus_gauge_value(&last_body, "swiftpipe_jobs_total", r#"status="failed""#,),
        0
    );
    assert_eq!(
        prometheus_gauge_value(&last_body, "swiftpipe_jobs_total", r#"status="stuck""#,),
        0
    );
}

#[tokio::test]
async fn metrics_prometheus_reports_messages_processed_by_type() {
    let (_tempdir, router) = test_router();

    let upload_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/upload?message_type=MT540&outputs=rendered")
                .body(Body::from(sample_fin()))
                .expect("upload request"),
        )
        .await
        .expect("upload response");
    assert_eq!(upload_response.status(), StatusCode::OK);

    let (status, _content_type, body) = request(router, Method::GET, "/metrics").await;
    let body = String::from_utf8(body).expect("utf8 metrics");

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(r#"swiftpipe_messages_processed_total{message_type="MT540"} 1"#));
}

#[tokio::test]
async fn metrics_prometheus_reports_parquet_bytes_written() {
    let (_tempdir, router) = test_router();

    let upload_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/upload?message_type=MT540&outputs=parquet")
                .body(Body::from(sample_fin()))
                .expect("upload request"),
        )
        .await
        .expect("upload response");
    assert_eq!(upload_response.status(), StatusCode::OK);

    let (status, _content_type, body) = request(router, Method::GET, "/metrics").await;
    let body = String::from_utf8(body).expect("utf8 metrics");
    let bytes_written = body
        .lines()
        .find_map(|line| {
            line.strip_prefix("swiftpipe_parquet_bytes_written_total ")
                .and_then(|value| value.parse::<u64>().ok())
        })
        .expect("parquet bytes metric");

    assert_eq!(status, StatusCode::OK);
    assert!(bytes_written > 0);
}

#[tokio::test]
async fn metrics_prometheus_reports_duckdb_rows_written() {
    let (_tempdir, router) = test_router();

    let upload_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/upload?message_type=MT540&outputs=parquet")
                .body(Body::from(sample_fin()))
                .expect("upload request"),
        )
        .await
        .expect("upload response");
    assert_eq!(upload_response.status(), StatusCode::OK);

    let (status, _content_type, body) = request(router, Method::GET, "/metrics").await;
    let body = String::from_utf8(body).expect("utf8 metrics");

    assert_eq!(status, StatusCode::OK);
    assert!(prometheus_unlabeled_value(&body, "swiftpipe_duckdb_rows_written_total") > 0.0);
}

#[tokio::test]
async fn metrics_prometheus_reports_object_store_ops_by_operation() {
    let (tempdir, router) = test_router();
    let input_uri = "s3://swiftpipe-inbox/prefix/input.fin";
    put_local_object(&tempdir, input_uri, &sample_fin());

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/jobs")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "input_prefix": "s3://swiftpipe-inbox/prefix/",
                        "message_type": "MT540",
                        "outputs": ["rendered"]
                    })
                    .to_string(),
                ))
                .expect("jobs request"),
        )
        .await
        .expect("jobs response");
    assert_eq!(response.status(), StatusCode::OK);

    let (status, _content_type, body) = request(router, Method::GET, "/metrics").await;
    let body = String::from_utf8(body).expect("utf8 metrics");

    assert_eq!(status, StatusCode::OK);
    assert!(
        prometheus_counter_value(&body, "swiftpipe_object_store_ops_total", r#"op="get""#,) > 0
    );
    assert!(
        prometheus_counter_value(&body, "swiftpipe_object_store_ops_total", r#"op="put""#,) > 0
    );
    assert!(
        prometheus_counter_value(&body, "swiftpipe_object_store_ops_total", r#"op="list""#,) > 0
    );
}

#[tokio::test]
async fn metrics_prometheus_reports_prefix_job_fanout_objects() {
    let (tempdir, router) = test_router();
    put_local_object(
        &tempdir,
        "s3://swiftpipe-inbox/prefix/first.fin",
        &sample_fin(),
    );
    put_local_object(
        &tempdir,
        "s3://swiftpipe-inbox/prefix/second.fin",
        &sample_fin(),
    );

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/jobs")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "input_prefix": "s3://swiftpipe-inbox/prefix/",
                        "message_type": "MT540",
                        "outputs": ["rendered"]
                    })
                    .to_string(),
                ))
                .expect("jobs request"),
        )
        .await
        .expect("jobs response");
    assert_eq!(response.status(), StatusCode::OK);

    let (status, _content_type, body) = request(router, Method::GET, "/metrics").await;
    let body = String::from_utf8(body).expect("utf8 metrics");

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        prometheus_unlabeled_value(&body, "swiftpipe_prefix_job_fanout_objects_count"),
        1.0
    );
    assert_eq!(
        prometheus_unlabeled_value(&body, "swiftpipe_prefix_job_fanout_objects_sum"),
        2.0
    );
}

#[tokio::test]
async fn metrics_prometheus_reports_api_errors_by_code() {
    let (_tempdir, router) = test_router();

    let error_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/jobs")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .expect("jobs request"),
        )
        .await
        .expect("jobs response");
    assert_eq!(error_response.status(), StatusCode::BAD_REQUEST);

    let (status, _content_type, body) = request(router, Method::GET, "/metrics").await;
    let body = String::from_utf8(body).expect("utf8 metrics");

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        prometheus_counter_value(&body, "swiftpipe_api_errors_total", r#"code="bad_request""#,),
        1
    );
}

#[tokio::test]
async fn metrics_json_returns_counter_snapshot() {
    let (_tempdir, router) = test_router();

    let (status, content_type, body) = request(router, Method::GET, "/metrics/json").await;
    let body: Value = serde_json::from_slice(&body).expect("json metrics");

    assert_eq!(status, StatusCode::OK);
    assert!(content_type.starts_with("application/json"));
    assert_eq!(body["jobs_submitted"], 0);
    assert_eq!(body["jobs_completed"], 0);
    assert_eq!(body["jobs_failed"], 0);
    assert_eq!(body["jobs_in_flight"], 0);
    assert_eq!(body["queue_depth"], 0);
}

#[tokio::test]
async fn metrics_rejects_unsupported_method() {
    let (_tempdir, router) = test_router();

    let (status, _content_type, _body) = request(router, Method::POST, "/metrics").await;

    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
}
