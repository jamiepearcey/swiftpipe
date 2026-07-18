use duckdb::Connection;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn render_command_validates_and_writes_fin_from_duckdb_rows() {
    let root = unique_temp_dir("swiftpipe-render-cli");
    fs::create_dir_all(&root).expect("creates temp dir");

    let db_path = root.join("swiftpipe.duckdb");
    let schema_path = root.join("schema.yaml");
    let config_path = root.join("swiftpipe.toml");
    let output_path = root.join("rendered.fin");

    write_schema(&schema_path);
    write_config(&config_path, &db_path, &schema_path);
    seed_duckdb(&db_path);

    run_swiftpipe(["migrate", "--config"], Some(&config_path), &root).assert_success();
    run_swiftpipe(["run", "--config"], Some(&config_path), &root).assert_success();
    run_swiftpipe(
        [
            "render",
            "--config",
            "--message-id",
            "msg-1",
            "--validate",
            "--output",
        ],
        Some(&config_path),
        &root,
    )
    .arg(&output_path)
    .assert_success();

    let rendered = fs::read_to_string(&output_path).expect("reads rendered FIN");
    assert!(rendered.contains("{1:F01BANKBEBBAXXX0000000000}"));
    assert!(rendered.contains("{2:I540BANKDEFFXXXXN}"));
    assert!(rendered.contains(":20C::SEME//ABC123"));
    assert!(rendered.contains(":98A::PREP//20260511"));

    fs::remove_dir_all(root).expect("removes temp dir");
}

#[test]
fn render_all_command_validates_and_writes_fin_files_from_duckdb_rows() {
    let root = unique_temp_dir("swiftpipe-render-all-cli");
    fs::create_dir_all(&root).expect("creates temp dir");

    let db_path = root.join("swiftpipe.duckdb");
    let schema_path = root.join("schema.yaml");
    let config_path = root.join("swiftpipe.toml");
    let output_dir = root.join("rendered");

    write_schema(&schema_path);
    write_config(&config_path, &db_path, &schema_path);
    seed_duckdb(&db_path);

    run_swiftpipe(["migrate", "--config"], Some(&config_path), &root).assert_success();
    run_swiftpipe(["run", "--config"], Some(&config_path), &root).assert_success();
    run_swiftpipe(
        ["render", "--config", "--all", "--validate", "--output-dir"],
        Some(&config_path),
        &root,
    )
    .arg(&output_dir)
    .assert_success();

    let first = fs::read_to_string(output_dir.join("msg-1.fin")).expect("reads first FIN");
    let second = fs::read_to_string(output_dir.join("msg-2.fin")).expect("reads second FIN");
    assert!(first.contains(":20C::SEME//ABC123"));
    assert!(second.contains(":20C::SEME//DEF456"));

    fs::remove_dir_all(root).expect("removes temp dir");
}

#[test]
fn schema_render_validate_reports_incomplete_outbound_metadata() {
    let root = unique_temp_dir("swiftpipe-render-validate-cli");
    fs::create_dir_all(&root).expect("creates temp dir");

    let schema_path = root.join("schema.yaml");
    write_incomplete_render_schema(&schema_path);

    let output = Command::new(env!("CARGO_BIN_EXE_swiftpipe"))
        .current_dir(&root)
        .args(["schema", "render-validate"])
        .arg(&schema_path)
        .output()
        .expect("runs swiftpipe");

    assert!(
        !output.status.success(),
        "render-validate should reject incomplete outbound metadata"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("MissingRenderOption"), "{stderr}");
    assert!(stderr.contains("MissingRenderQualifier"), "{stderr}");

    fs::remove_dir_all(root).expect("removes temp dir");
}

#[test]
fn render_command_rejects_duplicate_outbound_mappings_before_rendering() {
    let root = unique_temp_dir("swiftpipe-render-duplicate-mapping-cli");
    fs::create_dir_all(&root).expect("creates temp dir");

    let db_path = root.join("swiftpipe.duckdb");
    let schema_path = root.join("schema.yaml");
    let config_path = root.join("swiftpipe.toml");

    write_duplicate_mapping_schema(&schema_path);
    write_config(&config_path, &db_path, &schema_path);
    seed_duplicate_mapping_duckdb(&db_path);

    run_swiftpipe(["migrate", "--config"], Some(&config_path), &root).assert_success();
    run_swiftpipe(["run", "--config"], Some(&config_path), &root).assert_success();
    let output = run_swiftpipe(
        ["render", "--config", "--message-id", "msg-1"],
        Some(&config_path),
        &root,
    )
    .output()
    .expect("runs swiftpipe render");

    assert!(
        !output.status.success(),
        "render should reject duplicate outbound mappings"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("DuplicateRenderMapping"), "{stderr}");

    fs::remove_dir_all(root).expect("removes temp dir");
}

#[test]
fn render_command_rejects_ambiguous_message_type_lookup() {
    let root = unique_temp_dir("swiftpipe-render-ambiguous-type-cli");
    fs::create_dir_all(&root).expect("creates temp dir");

    let db_path = root.join("swiftpipe.duckdb");
    let schema_path = root.join("schema.yaml");
    let config_path = root.join("swiftpipe.toml");

    write_schema(&schema_path);
    write_config(&config_path, &db_path, &schema_path);
    seed_duckdb(&db_path);

    run_swiftpipe(["migrate", "--config"], Some(&config_path), &root).assert_success();
    run_swiftpipe(["run", "--config"], Some(&config_path), &root).assert_success();
    insert_raw_message(&db_path, "msg-1", "MT541", "{4:\n:16R:GENL\n:16S:GENL\n-}");

    let output = run_swiftpipe(
        ["render", "--config", "--message-id", "msg-1"],
        Some(&config_path),
        &root,
    )
    .output()
    .expect("runs swiftpipe render");

    assert!(
        !output.status.success(),
        "render should reject ambiguous message type lookup"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ambiguous message type for message id msg-1"),
        "{stderr}"
    );

    fs::remove_dir_all(root).expect("removes temp dir");
}

#[test]
fn render_command_reports_missing_message_type_lookup() {
    let root = unique_temp_dir("swiftpipe-render-missing-type-cli");
    fs::create_dir_all(&root).expect("creates temp dir");

    let db_path = root.join("swiftpipe.duckdb");
    let schema_path = root.join("schema.yaml");
    let config_path = root.join("swiftpipe.toml");

    write_schema(&schema_path);
    write_config(&config_path, &db_path, &schema_path);
    seed_duckdb(&db_path);

    run_swiftpipe(["migrate", "--config"], Some(&config_path), &root).assert_success();
    run_swiftpipe(["run", "--config"], Some(&config_path), &root).assert_success();

    let output = run_swiftpipe(
        ["render", "--config", "--message-id", "missing-msg"],
        Some(&config_path),
        &root,
    )
    .output()
    .expect("runs swiftpipe render");

    assert!(
        !output.status.success(),
        "render should reject unknown message ids"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("message type not found for message id missing-msg"),
        "{stderr}"
    );

    fs::remove_dir_all(root).expect("removes temp dir");
}

#[test]
fn render_command_does_not_overwrite_output_when_render_fails() {
    let root = unique_temp_dir("swiftpipe-render-output-failure-cli");
    fs::create_dir_all(&root).expect("creates temp dir");

    let db_path = root.join("swiftpipe.duckdb");
    let schema_path = root.join("schema.yaml");
    let config_path = root.join("swiftpipe.toml");
    let output_path = root.join("rendered.fin");

    write_schema(&schema_path);
    write_config(&config_path, &db_path, &schema_path);
    seed_duckdb(&db_path);

    run_swiftpipe(["migrate", "--config"], Some(&config_path), &root).assert_success();
    run_swiftpipe(["run", "--config"], Some(&config_path), &root).assert_success();
    delete_normalized_settlement_instruction(&db_path, "msg-1");
    fs::write(&output_path, "existing output").expect("writes existing output");

    let output = run_swiftpipe(
        [
            "render",
            "--config",
            "--message-id",
            "msg-1",
            "--validate",
            "--output",
        ],
        Some(&config_path),
        &root,
    )
    .arg(&output_path)
    .output()
    .expect("runs swiftpipe render");

    assert!(
        !output.status.success(),
        "render should fail when required rows are missing"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("failed to render msg-1 (MT540)"),
        "{stderr}"
    );
    assert!(stderr.contains("missing required render value"), "{stderr}");
    assert_eq!(
        fs::read_to_string(&output_path).expect("reads existing output"),
        "existing output",
        "render should not overwrite output when rendering fails"
    );

    fs::remove_dir_all(root).expect("removes temp dir");
}

#[test]
fn render_command_rejects_invalid_outbound_envelope_config() {
    let root = unique_temp_dir("swiftpipe-render-invalid-envelope-cli");
    fs::create_dir_all(&root).expect("creates temp dir");

    let db_path = root.join("swiftpipe.duckdb");
    let schema_path = root.join("schema.yaml");
    let config_path = root.join("swiftpipe.toml");

    write_schema(&schema_path);
    write_config_with_empty_block1(&config_path, &db_path, &schema_path);
    seed_duckdb(&db_path);

    run_swiftpipe(["migrate", "--config"], Some(&config_path), &root).assert_success();
    run_swiftpipe(["run", "--config"], Some(&config_path), &root).assert_success();
    let output = run_swiftpipe(
        ["render", "--config", "--message-id", "msg-1"],
        Some(&config_path),
        &root,
    )
    .output()
    .expect("runs swiftpipe render");

    assert!(
        !output.status.success(),
        "render should reject invalid outbound envelope config"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("outbound.block1 must not be empty"),
        "{stderr}"
    );

    fs::remove_dir_all(root).expect("removes temp dir");
}

#[test]
fn render_all_rejects_ambiguous_message_type_headers_before_writing_files() {
    let root = unique_temp_dir("swiftpipe-render-all-ambiguous-type-cli");
    fs::create_dir_all(&root).expect("creates temp dir");

    let db_path = root.join("swiftpipe.duckdb");
    let schema_path = root.join("schema.yaml");
    let config_path = root.join("swiftpipe.toml");
    let output_dir = root.join("rendered");

    write_schema(&schema_path);
    write_config(&config_path, &db_path, &schema_path);
    seed_duckdb(&db_path);

    run_swiftpipe(["migrate", "--config"], Some(&config_path), &root).assert_success();
    run_swiftpipe(["run", "--config"], Some(&config_path), &root).assert_success();
    insert_raw_message(&db_path, "msg-1", "MT541", "{4:\n:16R:GENL\n:16S:GENL\n-}");

    let output = run_swiftpipe(
        ["render", "--config", "--all", "--validate", "--output-dir"],
        Some(&config_path),
        &root,
    )
    .arg(&output_dir)
    .output()
    .expect("runs swiftpipe render");

    assert!(
        !output.status.success(),
        "render --all should reject ambiguous message headers"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ambiguous message type for message id msg-1"),
        "{stderr}"
    );
    assert!(
        output_dir
            .read_dir()
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(true),
        "render --all should not write files when headers are ambiguous"
    );

    fs::remove_dir_all(root).expect("removes temp dir");
}

#[test]
fn render_all_rejects_output_filename_collisions_before_writing_files() {
    let root = unique_temp_dir("swiftpipe-render-collision-cli");
    fs::create_dir_all(&root).expect("creates temp dir");

    let db_path = root.join("swiftpipe.duckdb");
    let schema_path = root.join("schema.yaml");
    let config_path = root.join("swiftpipe.toml");
    let output_dir = root.join("rendered");

    write_schema(&schema_path);
    write_config(&config_path, &db_path, &schema_path);
    seed_filename_collision_duckdb(&db_path);

    run_swiftpipe(["migrate", "--config"], Some(&config_path), &root).assert_success();
    run_swiftpipe(["run", "--config"], Some(&config_path), &root).assert_success();
    let output = run_swiftpipe(
        ["render", "--config", "--all", "--validate", "--output-dir"],
        Some(&config_path),
        &root,
    )
    .arg(&output_dir)
    .output()
    .expect("runs swiftpipe render");

    assert!(
        !output.status.success(),
        "render --all should reject colliding output names"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("multiple message ids map to the same output file"),
        "{stderr}"
    );
    assert!(
        !output_dir.join("msg_1.fin").exists(),
        "render --all should not write partial output when output names collide"
    );

    fs::remove_dir_all(root).expect("removes temp dir");
}

#[test]
fn render_all_does_not_write_partial_files_when_message_render_fails() {
    let root = unique_temp_dir("swiftpipe-render-all-render-failure-cli");
    fs::create_dir_all(&root).expect("creates temp dir");

    let db_path = root.join("swiftpipe.duckdb");
    let schema_path = root.join("schema.yaml");
    let config_path = root.join("swiftpipe.toml");
    let output_dir = root.join("rendered");

    write_schema(&schema_path);
    write_config(&config_path, &db_path, &schema_path);
    seed_duckdb(&db_path);

    run_swiftpipe(["migrate", "--config"], Some(&config_path), &root).assert_success();
    run_swiftpipe(["run", "--config"], Some(&config_path), &root).assert_success();
    delete_normalized_settlement_instruction(&db_path, "msg-2");

    let output = run_swiftpipe(
        ["render", "--config", "--all", "--validate", "--output-dir"],
        Some(&config_path),
        &root,
    )
    .arg(&output_dir)
    .output()
    .expect("runs swiftpipe render");

    assert!(
        !output.status.success(),
        "render --all should fail when one message cannot be rendered"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("failed to render msg-2 (MT540)"),
        "{stderr}"
    );
    assert!(stderr.contains("missing required render value"), "{stderr}");
    assert!(
        !output_dir.join("msg-1.fin").exists() && !output_dir.join("msg-2.fin").exists(),
        "render --all should not write partial output when one message fails"
    );

    fs::remove_dir_all(root).expect("removes temp dir");
}

trait CommandStatus {
    fn assert_success(&mut self);
}

impl CommandStatus for Command {
    fn assert_success(&mut self) {
        let output = self.output().expect("runs swiftpipe");
        assert!(
            output.status.success(),
            "swiftpipe failed\nstatus: {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn run_swiftpipe<const N: usize>(
    args: [&str; N],
    config_path: Option<&Path>,
    current_dir: &Path,
) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_swiftpipe"));
    command.current_dir(current_dir);
    for arg in args {
        command.arg(arg);
        if arg == "--config" {
            command.arg(config_path.expect("config path is required"));
        }
    }
    command
}

fn seed_duckdb(db_path: &Path) {
    let conn = Connection::open(db_path).expect("opens temp duckdb");
    conn.execute(
        "CREATE TABLE inbound_messages (
            id TEXT NOT NULL,
            message_type TEXT NOT NULL,
            body TEXT NOT NULL,
            processed BOOLEAN NOT NULL DEFAULT false
        )",
        [],
    )
    .expect("creates inbound table");
    conn.execute(
        "INSERT INTO inbound_messages (id, message_type, body) VALUES (?, ?, ?)",
        [
            "msg-1",
            "MT540",
            "{1:F01BANKBEBBAXXX0000000000}{2:I540BANKDEFFXXXXN}{4:\n:16R:GENL\n:20C::SEME//ABC123\n:98A::PREP//20260511\n:16S:GENL\n-}",
        ],
    )
    .expect("inserts inbound message");
    conn.execute(
        "INSERT INTO inbound_messages (id, message_type, body) VALUES (?, ?, ?)",
        [
            "msg-2",
            "MT540",
            "{1:F01BANKBEBBAXXX0000000000}{2:I540BANKDEFFXXXXN}{4:\n:16R:GENL\n:20C::SEME//DEF456\n:98A::PREP//20260512\n:16S:GENL\n-}",
        ],
    )
    .expect("inserts second inbound message");
}

fn seed_duplicate_mapping_duckdb(db_path: &Path) {
    let conn = Connection::open(db_path).expect("opens temp duckdb");
    conn.execute(
        "CREATE TABLE inbound_messages (
            id TEXT NOT NULL,
            message_type TEXT NOT NULL,
            body TEXT NOT NULL,
            processed BOOLEAN NOT NULL DEFAULT false
        )",
        [],
    )
    .expect("creates inbound table");
    conn.execute(
        "INSERT INTO inbound_messages (id, message_type, body) VALUES (?, ?, ?)",
        [
            "msg-1",
            "MT540",
            "{1:F01BANKBEBBAXXX0000000000}{2:I540BANKDEFFXXXXN}{4:\n:16R:SETDET\n:22F::STCO//NOMC\n:22H::REDE//DELI\n:16S:SETDET\n-}",
        ],
    )
    .expect("inserts inbound message");
}

fn seed_filename_collision_duckdb(db_path: &Path) {
    let conn = Connection::open(db_path).expect("opens temp duckdb");
    conn.execute(
        "CREATE TABLE inbound_messages (
            id TEXT NOT NULL,
            message_type TEXT NOT NULL,
            body TEXT NOT NULL,
            processed BOOLEAN NOT NULL DEFAULT false
        )",
        [],
    )
    .expect("creates inbound table");

    for (id, reference, date) in [
        ("msg/1", "ABC123", "20260511"),
        ("msg_1", "DEF456", "20260512"),
    ] {
        let body = format!(
            "{{1:F01BANKBEBBAXXX0000000000}}{{2:I540BANKDEFFXXXXN}}{{4:\n:16R:GENL\n:20C::SEME//{reference}\n:98A::PREP//{date}\n:16S:GENL\n-}}"
        );
        conn.execute(
            "INSERT INTO inbound_messages (id, message_type, body) VALUES (?, ?, ?)",
            [id, "MT540", body.as_str()],
        )
        .expect("inserts inbound message");
    }
}

fn insert_raw_message(db_path: &Path, message_id: &str, message_type: &str, raw_text: &str) {
    let conn = Connection::open(db_path).expect("opens temp duckdb");
    conn.execute(
        "INSERT INTO swift_raw_messages (message_id, message_type, raw_text) VALUES (?, ?, ?)",
        [message_id, message_type, raw_text],
    )
    .expect("inserts raw message");
}

fn delete_normalized_settlement_instruction(db_path: &Path, message_id: &str) {
    let conn = Connection::open(db_path).expect("opens temp duckdb");
    conn.execute(
        "DELETE FROM settlement_instruction WHERE message_id = ?",
        [message_id],
    )
    .expect("deletes normalized settlement instruction");
}

fn write_config(config_path: &Path, db_path: &Path, schema_path: &Path) {
    fs::write(
        config_path,
        format!(
            r#"[source]
adapter = "duckdb"
database = "{}"
table = "inbound_messages"
id_column = "id"
message_type_column = "message_type"
body_column = "body"
processed_column = "processed"

[sink]
adapter = "duckdb"
database = "{}"

[schemas]
paths = ["{}"]

[runtime]
batch_size = 100
fail_on_missing_schema = true
"#,
            toml_path(db_path),
            toml_path(db_path),
            toml_path(schema_path)
        ),
    )
    .expect("writes config");
}

fn write_config_with_empty_block1(config_path: &Path, db_path: &Path, schema_path: &Path) {
    fs::write(
        config_path,
        format!(
            r#"[source]
adapter = "duckdb"
database = "{}"
table = "inbound_messages"
id_column = "id"
message_type_column = "message_type"
body_column = "body"
processed_column = "processed"

[sink]
adapter = "duckdb"
database = "{}"

[schemas]
paths = ["{}"]

[runtime]
batch_size = 100
fail_on_missing_schema = true

[outbound]
block1 = ""
"#,
            toml_path(db_path),
            toml_path(db_path),
            toml_path(schema_path)
        ),
    )
    .expect("writes config");
}

fn write_schema(schema_path: &Path) {
    fs::write(
        schema_path,
        r#"field_types:
  - name: reference
    pattern:
      - kind: literal
        value: ":"
      - kind: capture
        name: qualifier
        value_type: alpha_num
        length: 4
      - kind: literal
        value: "//"
      - kind: rest
        name: value
  - name: date_yyyymmdd
    pattern:
      - kind: literal
        value: ":"
      - kind: capture
        name: qualifier
        value_type: alpha_num
        length: 4
      - kind: literal
        value: "//"
      - kind: capture
        name: date
        value_type: digits
        length: 8
messages:
  - message: MT540
    sequences:
      GENL: {}
    fields:
      - path: GENL
        tag: 20C
        qualifier: SEME
        name: sender_reference
        type: reference
        required: true
        entity: settlement_instruction
        column: sender_reference
      - path: GENL
        tag: 98a
        options: [A]
        qualifier: PREP
        name: preparation_date
        type: date_yyyymmdd
        required: true
        entity: settlement_instruction
        column: preparation_date
"#,
    )
    .expect("writes schema");
}

fn write_incomplete_render_schema(schema_path: &Path) {
    fs::write(
        schema_path,
        r#"field_types:
  - name: date_yyyymmdd
    pattern:
      - kind: literal
        value: ":"
      - kind: capture
        name: qualifier
        value_type: alpha_num
        length: 4
      - kind: literal
        value: "//"
      - kind: capture
        name: date
        value_type: digits
        length: 8
messages:
  - message: MT540
    sequences:
      GENL: {}
    fields:
      - path: GENL
        tag: 98a
        options: [A, C]
        name: preparation_date
        type: date_yyyymmdd
        entity: settlement_instruction
        column: preparation_date
"#,
    )
    .expect("writes incomplete render schema");
}

fn write_duplicate_mapping_schema(schema_path: &Path) {
    fs::write(
        schema_path,
        r#"field_types:
  - name: code
    pattern:
      - kind: literal
        value: ":"
      - kind: capture
        name: qualifier
        value_type: alpha_num
        length: 4
      - kind: literal
        value: "//"
      - kind: rest
        name: code
messages:
  - message: MT540
    sequences:
      SETDET: {}
    fields:
      - path: SETDET
        tag: 22F
        qualifier: STCO
        name: settlement_indicator
        type: code
        entity: settlement_indicator
        column: indicator
      - path: SETDET
        tag: 22H
        qualifier: REDE
        name: settlement_method
        type: code
        entity: settlement_indicator
        column: indicator
"#,
    )
    .expect("writes duplicate mapping schema");
}

fn toml_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "\\\\")
}

fn unique_temp_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "{}-{}-{}",
        name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ))
}
