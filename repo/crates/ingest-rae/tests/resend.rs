//! A fake engine over a real socket: a re-send of the same file must post the
//! same positions, and the engine's `already committed` 409 must read as landed.

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use ingest_rae::{ingest_file, HttpPoster, Options, SourceId};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Seen {
    path: String,
    position: String,
    content_type: String,
    magic: bool,
}

fn fake_engine() -> (String, Arc<Mutex<Vec<Seen>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let log: Arc<Mutex<Vec<Seen>>> = Default::default();
    let log2 = log.clone();
    std::thread::spawn(move || {
        let mut committed: BTreeSet<(String, String)> = BTreeSet::new();
        for stream in listener.incoming() {
            let mut s = stream.unwrap();
            let mut buf = Vec::new();
            let mut tmp = [0u8; 8192];
            let (head, mut body) = loop {
                let n = s.read(&mut tmp).unwrap();
                buf.extend_from_slice(&tmp[..n]);
                if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    break (
                        String::from_utf8_lossy(&buf[..i]).to_string(),
                        buf[i + 4..].to_vec(),
                    );
                }
            };
            let header = |name: &str| {
                head.lines()
                    .find_map(|l| {
                        let (k, v) = l.split_once(':')?;
                        k.eq_ignore_ascii_case(name).then(|| v.trim().to_string())
                    })
                    .unwrap_or_default()
            };
            let len: usize = header("content-length").parse().unwrap_or(0);
            while body.len() < len {
                let n = s.read(&mut tmp).unwrap();
                body.extend_from_slice(&tmp[..n]);
            }
            let path = head.lines().next().unwrap().split(' ').nth(1).unwrap().to_string();
            let position = header("x-rae-source-position");
            let table = path.split('?').next().unwrap().to_string();
            log2.lock().unwrap().push(Seen {
                path: path.clone(),
                position: position.clone(),
                content_type: header("content-type"),
                magic: body.starts_with(b"PAR1"),
            });
            let (code, text) = if committed.insert((table, position)) {
                ("201 Created", "{}")
            } else {
                ("409 Conflict", "source position was already committed at or before watermark")
            };
            let _ = write!(
                s,
                "HTTP/1.1 {code}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                text.len()
            );
        }
    });
    (base, log)
}

fn examples() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

#[test]
fn resend_posts_the_same_positions_and_lands_nothing_new() {
    let (base, log) = fake_engine();
    let ex = examples();
    let opts = Options::new(
        SourceId::parse("custodian:BANKGB22XXX").unwrap(),
        ex.join("schemas"),
    );
    let poster = HttpPoster::new(&base, Some("tok".into()));
    let bytes = std::fs::read(ex.join("mt940_sample.fin")).unwrap();

    let first = ingest_file(&poster, &opts, "mt940_sample.fin", None, &bytes).unwrap();
    let after_first = log.lock().unwrap().clone();
    let second = ingest_file(&poster, &opts, "mt940_sample.fin", None, &bytes).unwrap();
    let all = log.lock().unwrap().clone();
    let resend = &all[after_first.len()..];

    // five tables, raw last
    let paths: Vec<&str> = after_first.iter().map(|s| s.path.split('?').next().unwrap()).collect();
    assert_eq!(
        paths,
        vec!["/v1/msg/field", "/v1/msg/cash_statement", "/v1/msg/cash_entry", "/v1/msg/raw"]
    );
    assert!(after_first[0].path.ends_with("?tag=true&schema=strict"));
    assert!(after_first.iter().all(|s| s.magic && s.content_type == "application/vnd.apache.parquet"));
    // same positions on the re-send, in the same order
    assert_eq!(after_first, resend);
    assert!(after_first[0].position.starts_with("custodian.BANKGB22XXX."));
    assert!(after_first[0].position.ends_with("/0"), "{}", after_first[0].position);

    for (t, c) in &first.tables {
        assert_eq!((c.landed, c.already_landed), (1, 0), "{t}");
    }
    for (t, c) in &second.tables {
        assert_eq!((c.landed, c.already_landed), (0, 1), "{t}");
    }
    assert_eq!(first.tables["msg.cash_entry"].rows, 2);
    assert_eq!(first.tables["msg.raw"].rows, 1);
}

#[test]
fn a_non_idempotent_409_fails_the_file() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut s = stream.unwrap();
            let mut tmp = [0u8; 65536];
            let _ = s.read(&mut tmp);
            let body = "schema mismatch";
            let _ = write!(s, "HTTP/1.1 409 Conflict\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        }
    });
    let ex = examples();
    let opts = Options::new(SourceId::parse("custodian:BANKGB22XXX").unwrap(), ex.join("schemas"));
    let bytes = std::fs::read(ex.join("mt940_sample.fin")).unwrap();
    let err = ingest_file(&HttpPoster::new(&base, None), &opts, "mt940_sample.fin", None, &bytes).unwrap_err();
    assert!(format!("{err:#}").contains("409"), "{err:#}");
}
