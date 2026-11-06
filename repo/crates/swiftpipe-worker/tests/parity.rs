//! Parity with the `ingest-rae` sink over `repo/examples` (69 `.fin` + camt + csv), and the same
//! files through the real binary: inbox by `$ref` from a fake engine, multipart reply over the
//! worker's Unix socket.

#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::io::BufReader;
use std::net::TcpListener;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use arrow::array::{BinaryArray, RecordBatch, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use ingest_rae::{Options, Outcome, Poster, SourceId};
use rae_holon_worker::http::{parse_multipart, read_message, write_message};
use serde_json::{json, Value};
use swiftpipe_worker::{arrow_stream, InFile, Worker, TABLES};

fn ex() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn fixtures() -> Vec<InFile> {
    let mut names: Vec<String> = std::fs::read_dir(ex())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".fin") || n.ends_with(".xml") || n.ends_with(".csv"))
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|n| InFile {
            source_id: if n.ends_with(".csv") { "broker:sample" } else { "custodian:BANKBEBBXXX" }.into(),
            bytes: std::fs::read(ex().join(&n)).unwrap(),
            name: n,
        })
        .collect()
}

struct Count;
impl Poster for Count {
    fn post(&self, _: &str, _: Vec<u8>, _: &str) -> Result<Outcome> {
        Ok(Outcome::Landed)
    }
    fn post_artifact(&self, _: &[u8]) -> Result<String> {
        Ok(String::new())
    }
}

/// Rows per table as the sink's `post` counts them, and the files it refuses.
fn sink_rows(files: &[InFile]) -> (BTreeMap<String, u64>, Vec<String>) {
    let mut rows: BTreeMap<String, u64> = TABLES.iter().map(|t| (t.to_string(), 0)).collect();
    let mut refused = Vec::new();
    for f in files {
        let opts = Options::new(SourceId::parse(&f.source_id).unwrap(), ex().join("schemas"));
        match ingest_rae::ingest_file(&Count, &opts, &f.name, None, &f.bytes) {
            Ok(rep) => {
                for (t, c) in rep.tables {
                    *rows.get_mut(t).unwrap() += c.rows as u64;
                }
            }
            Err(_) => refused.push(f.name.clone()),
        }
    }
    (rows, refused)
}

fn inbox(files: &[InFile]) -> RecordBatch {
    let schema = Schema::new(vec![
        Field::new("source_id", DataType::Utf8, false),
        Field::new("name", DataType::Utf8, false),
        Field::new("bytes", DataType::Binary, false),
    ]);
    RecordBatch::try_new(
        Arc::new(schema),
        vec![
            Arc::new(StringArray::from_iter_values(files.iter().map(|f| f.source_id.as_str()))),
            Arc::new(StringArray::from_iter_values(files.iter().map(|f| f.name.as_str()))),
            Arc::new(BinaryArray::from_iter_values(files.iter().map(|f| f.bytes.as_slice()))),
        ],
    )
    .unwrap()
}

/// A one-route engine: `GET /v1/qw_mkt/ingress.inbox?version=7` with the bearer, as Arrow.
fn fake_engine(body: Vec<u8>, token: &'static str) -> (String, Arc<Mutex<Vec<String>>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let mut w = s.unwrap();
            let mut r = BufReader::new(w.try_clone().unwrap());
            while let Ok(Some(req)) = read_message(&mut r, 1 << 20) {
                let (m, p) = req.target();
                log.lock().unwrap().push(format!("{m} {p}"));
                let ok = p == "/v1/qw_mkt/ingress.inbox?version=7" && req.header("authorization") == Some(&format!("Bearer {token}"));
                let (code, b) = if ok { ("200 OK", body.as_slice()) } else { ("401 Unauthorized", &b"{}"[..]) };
                write_message(&mut w, &format!("HTTP/1.1 {code}"), &[("Content-Type", "application/vnd.apache.arrow.stream")], b).unwrap();
            }
        }
    });
    (base, seen)
}

#[test]
fn rows_per_table_equal_the_sink_and_the_binary_answers_them_by_reference() {
    let files = fixtures();
    assert_eq!(files.iter().filter(|f| f.name.ends_with(".fin")).count(), 69);
    let (want, sink_refused) = sink_rows(&files);
    assert_eq!(sink_refused, vec!["mt537_penalty_statement.csv".to_string()]);
    assert!(want["msg.raw"] > 69 && want["msg.field"] > 0 && want["msg.security_event"] > 0 && want["msg.cash_entry"] > 0, "{want:?}");

    // In process: one parse, five tables, the same rows.
    let worker = Worker::new(ex().join("schemas")).unwrap();
    assert_eq!(worker.schemas_loaded(), 212);
    let parsed = worker.parse(&files, &|| false).unwrap();
    let got: BTreeMap<String, u64> = parsed.tables.iter().map(|(t, b)| (t.to_string(), b.num_rows() as u64)).collect();
    assert_eq!(got, want);
    assert_eq!(parsed.output["refused"][0]["file"], "mt537_penalty_statement.csv");
    assert!(parsed.output["refused"][0]["reason"].as_str().unwrap().contains("unknown_layout"));
    assert!(matches!(worker.parse(&files, &|| true), Err(e) if e.to_string() == ingest_rae::CANCELLED));

    // Through the binary: inbox by $ref (string form), token on the read, multipart reply.
    let (engine, seen) = fake_engine(arrow_stream(&inbox(&files)).unwrap(), "tok-1");
    let sock = std::env::temp_dir().join(format!("spw-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&sock);
    let mut child = Command::new(env!("CARGO_BIN_EXE_swiftpipe-worker"))
        .env("HOLON_LISTEN", &sock)
        .env("HOLON_TOKEN", "tok-1")
        .env("HOLON_ENGINE", &engine)
        .env("HOLON_KIND", "holon:qw_mkt.swift_parse")
        .env("SWIFTPIPE_SCHEMAS", ex().join("schemas"))
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let t0 = Instant::now();
    let conn = loop {
        if let Ok(c) = UnixStream::connect(&sock) {
            break c;
        }
        assert!(t0.elapsed() < Duration::from_secs(20), "the worker never listened");
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut w = conn.try_clone().unwrap();
    let mut r = BufReader::new(conn);
    let ask = json!({"key": "k-1", "attempt": 1, "deadline_unix_ms": 0, "run": {}, "reply_schema": null,
        "input": {"files": {"$ref": "/v1/qw_mkt/ingress.inbox@v7"}}});
    let auth = [("Authorization", "Bearer tok-1"), ("Content-Type", "application/json")];
    write_message(&mut w, "POST /step/swift_parse HTTP/1.1", &auth, ask.to_string().as_bytes()).unwrap();
    let resp = read_message(&mut r, 1 << 30).unwrap().unwrap();
    assert_eq!(resp.status(), Some(200));
    let parts = parse_multipart(resp.header("content-type").unwrap(), &resp.body).unwrap();
    let doc: Value = serde_json::from_slice(&parts[0].body).unwrap();
    assert_eq!(doc["status"], "ok", "{doc}");
    assert_eq!(parts.len(), 1 + TABLES.len());
    let mut wire = BTreeMap::new();
    for (i, t) in TABLES.iter().enumerate() {
        let p = &parts[i + 1];
        assert_eq!(p.header("x-rae-table"), Some(*t));
        assert_eq!(doc["tables"][i]["table"], *t);
        let rows: usize = arrow::ipc::reader::StreamReader::try_new(std::io::Cursor::new(&p.body), None).unwrap().map(|b| b.unwrap().num_rows()).sum();
        assert_eq!(doc["tables"][i]["rows"].as_u64(), Some(rows as u64));
        wire.insert(t.to_string(), rows as u64);
    }
    assert_eq!(wire, want);
    assert_eq!(seen.lock().unwrap().as_slice(), ["GET /v1/qw_mkt/ingress.inbox?version=7"]);

    // A finished attempt polls as done; closing the last connection ends the process.
    write_message(&mut w, "GET /attempts/k-1 HTTP/1.1", &auth[..1], b"").unwrap();
    let poll: Value = serde_json::from_slice(&read_message(&mut r, 1 << 20).unwrap().unwrap().body).unwrap();
    assert_eq!(poll["state"], "done");
    drop((w, r));
    let t0 = Instant::now();
    while child.try_wait().unwrap().is_none() {
        assert!(t0.elapsed() < Duration::from_secs(10), "the worker outlived its last connection");
        std::thread::sleep(Duration::from_millis(20));
    }
}
