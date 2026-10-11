use duckdb::Connection;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const RJE: &str =
    "../../examples/public-swift-samples/upstream/prowide-core_MT103-bulk-with-ack.rje";
const FIN: &str = "../../examples/mt537_sample.fin";
const SYNTH: &str = "{1:F01AAAAGB2LAXXX0000000001}{2:I202BBBBGB2LXXXXN}{4:\n:20:B\n-}";

struct Env {
    root: PathBuf,
    config: PathBuf,
    db: PathBuf,
}

fn manifest(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn setup(name: &str, table: &str) -> Env {
    let root = std::env::temp_dir().join(format!(
        "swiftpipe-load-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let db = root.join("swiftpipe.duckdb");
    let config = root.join("swiftpipe.toml");
    let schemas = manifest("../../examples/schemas");
    let esc = |p: &Path| p.to_string_lossy().replace('\\', "\\\\");
    fs::write(
        &config,
        format!(
            r#"[source]
adapter = "duckdb"
database = "{db}"
table = "{table}"
id_column = "id"
message_type_column = "message_type"
body_column = "body"
processed_column = "processed"

[sink]
adapter = "duckdb"
database = "{db}"
write_batch_size = 1000

[schemas]
paths = ["{schemas}"]

[runtime]
batch_size = 10000
fail_on_missing_schema = false
"#,
            db = esc(&db),
            schemas = esc(&schemas),
        ),
    )
    .unwrap();
    Env { root, config, db }
}

fn swiftpipe(env: &Env, sub: &str, files: &[&Path]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_swiftpipe"))
        .current_dir(&env.root)
        .arg(sub)
        .arg("--config")
        .arg(&env.config)
        .args(files)
        .output()
        .expect("runs swiftpipe")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn rows(env: &Env) -> Vec<(String, String)> {
    let conn = Connection::open(&env.db).unwrap();
    let mut stmt = conn
        .prepare("SELECT id, message_type FROM inbound_messages ORDER BY id")
        .unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn load_multi_message_rje_inserts_each_message() {
    let env = setup("rje", "inbound_messages");
    let rje = manifest(RJE);
    let out = swiftpipe(&env, "load", &[&rje]);
    assert!(out.status.success(), "{out:?}");
    assert!(
        stdout(&out).contains("loaded 3 message(s)"),
        "{}",
        stdout(&out)
    );
    let stem = "prowide-core_MT103-bulk-with-ack";
    let expected: Vec<_> = (1..=3)
        .map(|n| (format!("{stem}-{n}"), "MT103".to_string()))
        .collect();
    assert_eq!(rows(&env), expected);
    fs::remove_dir_all(&env.root).unwrap();
}

#[test]
fn load_mixed_files_reports_summed_count() {
    let env = setup("mixed", "inbound_messages");
    let synth = env.root.join("synthetic.fin");
    fs::write(&synth, SYNTH).unwrap();
    let (rje, fin) = (manifest(RJE), manifest(FIN));
    let out = swiftpipe(&env, "load", &[&rje, &fin, &synth]);
    assert!(out.status.success(), "{out:?}");
    assert!(
        stdout(&out).contains("loaded 5 message(s)"),
        "{}",
        stdout(&out)
    );
    let r = rows(&env);
    assert_eq!(r.len(), 5);
    assert!(r.contains(&("mt537_sample-1".into(), "MT537".into())));
    assert!(r.contains(&("synthetic-1".into(), "MT202".into())));
    fs::remove_dir_all(&env.root).unwrap();
}

#[test]
fn load_file_without_fin_messages_loads_zero_and_succeeds() {
    let env = setup("empty", "inbound_messages");
    let junk = env.root.join("junk.txt");
    fs::write(&junk, "no messages here").unwrap();
    let out = swiftpipe(&env, "load", &[&junk]);
    assert!(out.status.success(), "{out:?}");
    assert!(stdout(&out).contains("loaded 0 message(s)"));
    assert!(rows(&env).is_empty());
    fs::remove_dir_all(&env.root).unwrap();
}

#[test]
fn load_twice_appends_duplicate_ids() {
    // Documented behaviour: no de-duplication or upsert; a second load appends
    // the same ids again (the table has no primary key).
    let env = setup("twice", "inbound_messages");
    let rje = manifest(RJE);
    assert!(swiftpipe(&env, "load", &[&rje]).status.success());
    assert!(swiftpipe(&env, "load", &[&rje]).status.success());
    let r = rows(&env);
    assert_eq!(r.len(), 6);
    assert_eq!(r.iter().filter(|(id, _)| id.ends_with("-1")).count(), 2);
    fs::remove_dir_all(&env.root).unwrap();
}

#[test]
fn load_rejects_unsafe_identifier() {
    let env = setup("unsafe", "x; DROP TABLE y");
    let rje = manifest(RJE);
    let out = swiftpipe(&env, "load", &[&rje]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("unsafe identifier"));
    fs::remove_dir_all(&env.root).unwrap();
}

#[test]
fn load_then_run_processes_every_message_without_parse_errors() {
    let env = setup("run", "inbound_messages");
    let (rje, fin) = (manifest(RJE), manifest(FIN));
    assert!(swiftpipe(&env, "load", &[&rje, &fin]).status.success());
    let out = swiftpipe(&env, "run", &[]);
    assert!(out.status.success(), "{out:?}");
    let s = stdout(&out);
    assert!(s.contains("processed 4 message(s)"), "{s}");
    assert!(s.contains("0 parse error row(s)"), "{s}");
    let conn = Connection::open(&env.db).unwrap();
    let done: i64 = conn
        .query_row(
            "SELECT count(*) FROM inbound_messages WHERE processed",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(done, 4);
    fs::remove_dir_all(&env.root).unwrap();
}

#[test]
fn load_bare_statements_wraps_and_types_each_statement() {
    let env = setup("bare", "inbound_messages");
    let sta = manifest("../../examples/public-swift-samples/upstream/wolph-mt940_jejik_ing.sta");
    let mt942 =
        manifest("../../examples/public-swift-samples/upstream/wolph-mt940_mBank_mt942.sta");
    let out = swiftpipe(&env, "load", &[&sta, &mt942]);
    assert!(out.status.success(), "{out:?}");
    let got = rows(&env);
    assert!(got.iter().any(|(_, t)| t == "MT940"), "{got:?}");
    assert!(got.iter().any(|(_, t)| t == "MT942"), "{got:?}");
    let run = swiftpipe(&env, "run", &[]);
    assert!(run.status.success(), "{run:?}");
    assert!(
        stdout(&run).contains("0 parse error row(s)"),
        "{}",
        stdout(&run)
    );
    fs::remove_dir_all(&env.root).unwrap();
}
