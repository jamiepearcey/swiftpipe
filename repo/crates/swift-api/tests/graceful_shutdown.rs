use axum::http::StatusCode;
use serde_json::Value;
use std::net::SocketAddr;
use std::time::Duration;
use swift_api::make_queued_router_for_test;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

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

async fn post_upload(addr: SocketAddr, body: &[u8]) -> (StatusCode, Value) {
    let mut stream = TcpStream::connect(addr).await.expect("connect server");
    let request = format!(
        "POST /v1/upload?message_type=MT540&outputs=rendered HTTP/1.1\r\n\
         Host: {addr}\r\n\
         Connection: close\r\n\
         Content-Length: {}\r\n\
         \r\n",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("write request head");
    stream.write_all(body).await.expect("write request body");

    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .await
        .expect("read response");
    parse_response(&response)
}

fn parse_response(response: &[u8]) -> (StatusCode, Value) {
    let response = std::str::from_utf8(response).expect("utf8 response");
    let (head, body) = response
        .split_once("\r\n\r\n")
        .expect("response head/body separator");
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .and_then(|code| StatusCode::from_u16(code).ok())
        .expect("status code");
    let body = serde_json::from_str(body).expect("json response body");
    (status, body)
}

#[tokio::test]
async fn graceful_shutdown_drains_in_flight_jobs() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let object_root = tempdir.path().join("objects");
    let work_root = tempdir.path().join("work");
    std::fs::create_dir_all(&object_root).expect("object root");
    std::fs::create_dir_all(&work_root).expect("work root");

    let (router, mut workers) = make_queued_router_for_test(
        workspace_root().join("examples/schemas"),
        object_root,
        work_root,
        100 * 1024 * 1024,
        1,
        8,
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = shutdown_rx.await;
        })
        .await
    });

    let body = sample_fin();
    let mut job_ids = Vec::new();
    for _ in 0..3 {
        let (status, response) = post_upload(addr, &body).await;
        assert_eq!(status, StatusCode::ACCEPTED);
        job_ids.push(
            response["job_id"]
                .as_str()
                .expect("job_id string")
                .to_string(),
        );
    }

    shutdown_tx.send(()).expect("send shutdown");
    tokio::time::timeout(Duration::from_secs(30), server)
        .await
        .expect("server exits within 30s")
        .expect("server join")
        .expect("server result");
    workers
        .drain(Duration::from_secs(30))
        .await
        .expect("workers drain");

    let jobs = workers.list_jobs(10);
    for job_id in job_ids {
        let status = jobs
            .iter()
            .find(|job| job.job_id == job_id)
            .map(|job| job.status.as_str())
            .expect("submitted job stored");
        assert!(
            matches!(status, "completed" | "failed"),
            "job {job_id} remained non-terminal: {status}"
        );
    }
}
