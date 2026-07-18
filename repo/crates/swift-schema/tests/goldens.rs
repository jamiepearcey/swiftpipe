use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use swift_core::{parse_message, SequenceFrame};
use swift_schema::{match_and_parse_message, CapturedFieldValue, SchemaCatalog};

struct GoldenCase {
    message_type: &'static str,
    sample_file: &'static str,
    snapshot_file: &'static str,
}

const GOLDEN_CASES: &[GoldenCase] = &[
    GoldenCase {
        message_type: "MT321",
        sample_file: "mt321_sample.fin",
        snapshot_file: "mt321.ndjson",
    },
    GoldenCase {
        message_type: "MT370",
        sample_file: "mt370_sample.fin",
        snapshot_file: "mt370.ndjson",
    },
    GoldenCase {
        message_type: "MT380",
        sample_file: "mt380_sample.fin",
        snapshot_file: "mt380.ndjson",
    },
    GoldenCase {
        message_type: "MT381",
        sample_file: "mt381_sample.fin",
        snapshot_file: "mt381.ndjson",
    },
    GoldenCase {
        message_type: "MT500",
        sample_file: "mt500_sample.fin",
        snapshot_file: "mt500.ndjson",
    },
    GoldenCase {
        message_type: "MT501",
        sample_file: "mt501_sample.fin",
        snapshot_file: "mt501.ndjson",
    },
    GoldenCase {
        message_type: "MT502",
        sample_file: "mt502_sample.fin",
        snapshot_file: "mt502.ndjson",
    },
    GoldenCase {
        message_type: "MT503",
        sample_file: "mt503_sample.fin",
        snapshot_file: "mt503.ndjson",
    },
    GoldenCase {
        message_type: "MT504",
        sample_file: "mt504_sample.fin",
        snapshot_file: "mt504.ndjson",
    },
    GoldenCase {
        message_type: "MT505",
        sample_file: "mt505_sample.fin",
        snapshot_file: "mt505.ndjson",
    },
    GoldenCase {
        message_type: "MT506",
        sample_file: "mt506_sample.fin",
        snapshot_file: "mt506.ndjson",
    },
    GoldenCase {
        message_type: "MT507",
        sample_file: "mt507_sample.fin",
        snapshot_file: "mt507.ndjson",
    },
    GoldenCase {
        message_type: "MT508",
        sample_file: "mt508_sample.fin",
        snapshot_file: "mt508.ndjson",
    },
    GoldenCase {
        message_type: "MT509",
        sample_file: "mt509_sample.fin",
        snapshot_file: "mt509.ndjson",
    },
    GoldenCase {
        message_type: "MT510",
        sample_file: "mt510_sample.fin",
        snapshot_file: "mt510.ndjson",
    },
    GoldenCase {
        message_type: "MT513",
        sample_file: "mt513_sample.fin",
        snapshot_file: "mt513.ndjson",
    },
    GoldenCase {
        message_type: "MT514",
        sample_file: "mt514_sample.fin",
        snapshot_file: "mt514.ndjson",
    },
    GoldenCase {
        message_type: "MT515",
        sample_file: "mt515_sample.fin",
        snapshot_file: "mt515.ndjson",
    },
    GoldenCase {
        message_type: "MT516",
        sample_file: "mt516_sample.fin",
        snapshot_file: "mt516.ndjson",
    },
    GoldenCase {
        message_type: "MT517",
        sample_file: "mt517_sample.fin",
        snapshot_file: "mt517.ndjson",
    },
    GoldenCase {
        message_type: "MT518",
        sample_file: "mt518_sample.fin",
        snapshot_file: "mt518.ndjson",
    },
    GoldenCase {
        message_type: "MT519",
        sample_file: "mt519_sample.fin",
        snapshot_file: "mt519.ndjson",
    },
    GoldenCase {
        message_type: "MT524",
        sample_file: "mt524_sample.fin",
        snapshot_file: "mt524.ndjson",
    },
    GoldenCase {
        message_type: "MT526",
        sample_file: "mt526_sample.fin",
        snapshot_file: "mt526.ndjson",
    },
    GoldenCase {
        message_type: "MT527",
        sample_file: "mt527_sample.fin",
        snapshot_file: "mt527.ndjson",
    },
    GoldenCase {
        message_type: "MT530",
        sample_file: "mt530_sample.fin",
        snapshot_file: "mt530.ndjson",
    },
    GoldenCase {
        message_type: "MT535",
        sample_file: "mt535_sample.fin",
        snapshot_file: "mt535.ndjson",
    },
    GoldenCase {
        message_type: "MT536",
        sample_file: "mt536_sample.fin",
        snapshot_file: "mt536.ndjson",
    },
    GoldenCase {
        message_type: "MT537",
        sample_file: "mt537_sample.fin",
        snapshot_file: "mt537.ndjson",
    },
    GoldenCase {
        message_type: "MT538",
        sample_file: "mt538_sample.fin",
        snapshot_file: "mt538.ndjson",
    },
    GoldenCase {
        message_type: "MT540",
        sample_file: "mt540_sample.fin",
        snapshot_file: "mt540.ndjson",
    },
    GoldenCase {
        message_type: "MT541",
        sample_file: "mt541_sample.fin",
        snapshot_file: "mt541.ndjson",
    },
    GoldenCase {
        message_type: "MT542",
        sample_file: "mt542_sample.fin",
        snapshot_file: "mt542.ndjson",
    },
    GoldenCase {
        message_type: "MT543",
        sample_file: "mt543_sample.fin",
        snapshot_file: "mt543.ndjson",
    },
    GoldenCase {
        message_type: "MT544",
        sample_file: "mt544_sample.fin",
        snapshot_file: "mt544.ndjson",
    },
    GoldenCase {
        message_type: "MT545",
        sample_file: "mt545_sample.fin",
        snapshot_file: "mt545.ndjson",
    },
    GoldenCase {
        message_type: "MT546",
        sample_file: "mt546_sample.fin",
        snapshot_file: "mt546.ndjson",
    },
    GoldenCase {
        message_type: "MT547",
        sample_file: "mt547_sample.fin",
        snapshot_file: "mt547.ndjson",
    },
    GoldenCase {
        message_type: "MT548",
        sample_file: "mt548_sample.fin",
        snapshot_file: "mt548.ndjson",
    },
    GoldenCase {
        message_type: "MT549",
        sample_file: "mt549_sample.fin",
        snapshot_file: "mt549.ndjson",
    },
    GoldenCase {
        message_type: "MT558",
        sample_file: "mt558_sample.fin",
        snapshot_file: "mt558.ndjson",
    },
    GoldenCase {
        message_type: "MT564",
        sample_file: "mt564_sample.fin",
        snapshot_file: "mt564.ndjson",
    },
    GoldenCase {
        message_type: "MT565",
        sample_file: "mt565_sample.fin",
        snapshot_file: "mt565.ndjson",
    },
    GoldenCase {
        message_type: "MT566",
        sample_file: "mt566_sample.fin",
        snapshot_file: "mt566.ndjson",
    },
    GoldenCase {
        message_type: "MT567",
        sample_file: "mt567_sample.fin",
        snapshot_file: "mt567.ndjson",
    },
    GoldenCase {
        message_type: "MT568",
        sample_file: "mt568_sample.fin",
        snapshot_file: "mt568.ndjson",
    },
    GoldenCase {
        message_type: "MT569",
        sample_file: "mt569_sample.fin",
        snapshot_file: "mt569.ndjson",
    },
    GoldenCase {
        message_type: "MT575",
        sample_file: "mt575_sample.fin",
        snapshot_file: "mt575.ndjson",
    },
    GoldenCase {
        message_type: "MT576",
        sample_file: "mt576_sample.fin",
        snapshot_file: "mt576.ndjson",
    },
    GoldenCase {
        message_type: "MT578",
        sample_file: "mt578_sample.fin",
        snapshot_file: "mt578.ndjson",
    },
    GoldenCase {
        message_type: "MT581",
        sample_file: "mt581_sample.fin",
        snapshot_file: "mt581.ndjson",
    },
    GoldenCase {
        message_type: "MT586",
        sample_file: "mt586_sample.fin",
        snapshot_file: "mt586.ndjson",
    },
    GoldenCase {
        message_type: "MT590",
        sample_file: "mt590_sample.fin",
        snapshot_file: "mt590.ndjson",
    },
    GoldenCase {
        message_type: "MT591",
        sample_file: "mt591_sample.fin",
        snapshot_file: "mt591.ndjson",
    },
    GoldenCase {
        message_type: "MT592",
        sample_file: "mt592_sample.fin",
        snapshot_file: "mt592.ndjson",
    },
    GoldenCase {
        message_type: "MT595",
        sample_file: "mt595_sample.fin",
        snapshot_file: "mt595.ndjson",
    },
    GoldenCase {
        message_type: "MT596",
        sample_file: "mt596_sample.fin",
        snapshot_file: "mt596.ndjson",
    },
    GoldenCase {
        message_type: "MT598",
        sample_file: "mt598_sample.fin",
        snapshot_file: "mt598.ndjson",
    },
    GoldenCase {
        message_type: "MT599",
        sample_file: "mt599_sample.fin",
        snapshot_file: "mt599.ndjson",
    },
    GoldenCase {
        message_type: "MT670",
        sample_file: "mt670_sample.fin",
        snapshot_file: "mt670.ndjson",
    },
    GoldenCase {
        message_type: "MT671",
        sample_file: "mt671_sample.fin",
        snapshot_file: "mt671.ndjson",
    },
];

fn repo_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("crate should live under the workspace crates directory")
        .to_path_buf()
}

fn load_catalog() -> SchemaCatalog {
    let schema_dir = repo_root().join("examples/schemas");
    let mut entries = fs::read_dir(schema_dir)
        .expect("schemas directory should be readable")
        .map(|entry| entry.expect("schema entry should be readable").path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("yaml"))
        .collect::<Vec<_>>();
    entries.sort();

    let mut field_types = Vec::new();
    let mut messages = Vec::new();
    for path in entries {
        let source = fs::read_to_string(path).expect("schema should be readable");
        let mut schema = SchemaCatalog::from_yaml_str(&source).expect("schema should parse");
        for field_type in schema.field_types.drain(..) {
            if let Some(existing) = field_types
                .iter()
                .find(|existing: &&swift_schema::FieldTypeSchema| existing.name == field_type.name)
            {
                assert_eq!(
                    existing, &field_type,
                    "duplicate field type definitions must match"
                );
                continue;
            }
            field_types.push(field_type);
        }
        messages.append(&mut schema.messages);
    }

    let catalog = SchemaCatalog {
        field_types,
        messages,
    };
    catalog
        .validate_rendering()
        .expect("example schema catalog should validate for rendering");
    catalog
}

fn load_sample(name: &str) -> Vec<u8> {
    fs::read(repo_root().join("examples").join(name)).expect("sample should be readable")
}

fn snapshot_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("goldens")
}

fn assert_golden(case: &GoldenCase) {
    let actual = normalized_snapshot(case);
    let snapshot_path = snapshot_dir().join(case.snapshot_file);

    if std::env::var_os("UPDATE_GOLDENS").is_some() {
        fs::create_dir_all(snapshot_dir()).expect("golden snapshot directory should be writable");
        fs::write(&snapshot_path, &actual).expect("golden snapshot should be writable");
        return;
    }

    let expected = fs::read_to_string(&snapshot_path).expect("golden snapshot should exist");
    assert_eq!(
        expected, actual,
        "golden snapshot drifted; run UPDATE_GOLDENS=1 cargo test -p swift-schema to refresh"
    );
}

fn normalized_snapshot(case: &GoldenCase) -> String {
    let catalog = load_catalog();
    let schema = catalog
        .message(case.message_type)
        .expect("message schema should exist");
    let input = load_sample(case.sample_file);
    let parsed = parse_message(&input);
    assert!(
        parsed.diagnostics.is_empty(),
        "sample should parse without core diagnostics: {:?}",
        parsed.diagnostics
    );
    let parsed_match = match_and_parse_message(&catalog, schema, &parsed);
    assert!(
        parsed_match.parse_errors.is_empty(),
        "sample fields should parse without schema errors: {:?}",
        parsed_match.parse_errors
    );
    assert!(
        parsed_match.missing_required.is_empty(),
        "sample should include required fields: {:?}",
        parsed_match.missing_required
    );
    assert!(
        parsed_match.cardinality_violations.is_empty(),
        "sample should satisfy field cardinality: {:?}",
        parsed_match.cardinality_violations
    );
    assert!(
        parsed_match.sequence_issues.is_empty(),
        "sample should satisfy sequence rules: {:?}",
        parsed_match.sequence_issues
    );

    let mut records = parsed_match
        .matched_fields
        .iter()
        .map(|matched| {
            let mut record = BTreeMap::new();
            record.insert(
                "column".to_string(),
                Value::String(matched.rule.column.clone()),
            );
            record.insert(
                "entity".to_string(),
                Value::String(matched.rule.entity.clone()),
            );
            record.insert(
                "field_type".to_string(),
                Value::String(matched.field_type.to_string()),
            );
            record.insert("name".to_string(), Value::String(matched.rule.name.clone()));
            record.insert("path".to_string(), Value::String(matched.rule.path.clone()));
            record.insert(
                "qualifier".to_string(),
                matched
                    .field
                    .qualifier
                    .map(bytes_to_json_string)
                    .unwrap_or(Value::Null),
            );
            record.insert(
                "sequence_path".to_string(),
                Value::String(
                    sequence_path_to_string(&matched.field.sequence_path)
                        .unwrap_or_else(|| "$".to_string()),
                ),
            );
            record.insert(
                "tag".to_string(),
                Value::String(bytes_to_string(matched.field.tag)),
            );
            record.insert(
                "value".to_string(),
                Value::String(normalize_capture_value(
                    matched.field_type,
                    selected_capture_value(&matched.captures),
                )),
            );
            record.insert(
                "captures".to_string(),
                Value::Object(
                    matched
                        .captures
                        .iter()
                        .map(|capture| {
                            (
                                capture.name.clone(),
                                Value::String(bytes_to_string(capture.value)),
                            )
                        })
                        .collect(),
                ),
            );
            serde_json::to_string(&record).expect("snapshot record should serialize")
        })
        .collect::<Vec<_>>();
    records.sort();
    format!("{}\n", records.join("\n"))
}

fn selected_capture_value(captures: &[CapturedFieldValue<'_>]) -> String {
    for preferred in ["value", "date", "amount", "quantity", "code"] {
        if let Some(capture) = captures.iter().find(|capture| capture.name == preferred) {
            return bytes_to_string(capture.value);
        }
    }

    captures
        .last()
        .map(|capture| bytes_to_string(capture.value))
        .unwrap_or_default()
}

fn normalize_capture_value(field_type: &str, value: String) -> String {
    if field_type.contains("date")
        && value.len() == 8
        && value.bytes().all(|byte| byte.is_ascii_digit())
    {
        format!("{}-{}-{}", &value[0..4], &value[4..6], &value[6..8])
    } else if field_type.contains("amount") {
        normalize_swift_amount(&value)
    } else if field_type.contains("quantity") {
        normalize_swift_quantity(&value)
    } else {
        value
    }
}

fn normalize_swift_amount(value: &str) -> String {
    let unsigned = value.strip_prefix('N').unwrap_or(value);
    let numeric =
        if unsigned.len() > 3 && unsigned[..3].bytes().all(|byte| byte.is_ascii_uppercase()) {
            &unsigned[3..]
        } else {
            unsigned
        };
    let normalized = numeric.replace(',', ".");
    if value.starts_with('N') {
        format!("-{normalized}")
    } else {
        normalized
    }
}

fn normalize_swift_quantity(value: &str) -> String {
    value
        .rsplit_once('/')
        .map_or(value, |(_, amount)| amount)
        .replace(',', ".")
}

fn sequence_path_to_string(path: &[SequenceFrame<'_>]) -> Option<String> {
    if path.is_empty() {
        return None;
    }

    Some(
        path.iter()
            .map(|frame| format!("{}[{}]", bytes_to_string(frame.name), frame.occurrence))
            .collect::<Vec<_>>()
            .join("/"),
    )
}

fn bytes_to_string(value: &[u8]) -> String {
    String::from_utf8_lossy(value).into_owned()
}

fn bytes_to_json_string(value: &[u8]) -> Value {
    Value::String(bytes_to_string(value))
}

#[test]
fn mt321_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[0]);
}

#[test]
fn mt370_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[1]);
}

#[test]
fn mt380_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[2]);
}

#[test]
fn mt381_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[3]);
}

#[test]
fn mt500_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[4]);
}

#[test]
fn mt501_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[5]);
}

#[test]
fn mt502_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[6]);
}

#[test]
fn mt503_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[7]);
}

#[test]
fn mt504_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[8]);
}

#[test]
fn mt505_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[9]);
}

#[test]
fn mt506_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[10]);
}

#[test]
fn mt507_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[11]);
}

#[test]
fn mt508_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[12]);
}

#[test]
fn mt509_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[13]);
}

#[test]
fn mt510_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[14]);
}

#[test]
fn mt513_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[15]);
}

#[test]
fn mt514_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[16]);
}

#[test]
fn mt515_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[17]);
}

#[test]
fn mt516_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[18]);
}

#[test]
fn mt517_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[19]);
}

#[test]
fn mt518_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[20]);
}

#[test]
fn mt519_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[21]);
}

#[test]
fn mt524_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[22]);
}

#[test]
fn mt526_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[23]);
}

#[test]
fn mt527_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[24]);
}

#[test]
fn mt530_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[25]);
}

#[test]
fn mt535_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[26]);
}

#[test]
fn mt536_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[27]);
}

#[test]
fn mt537_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[28]);
}

#[test]
fn mt538_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[29]);
}

#[test]
fn mt540_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[30]);
}

#[test]
fn mt541_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[31]);
}

#[test]
fn mt542_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[32]);
}

#[test]
fn mt543_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[33]);
}

#[test]
fn mt544_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[34]);
}

#[test]
fn mt545_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[35]);
}

#[test]
fn mt546_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[36]);
}

#[test]
fn mt547_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[37]);
}

#[test]
fn mt548_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[38]);
}

#[test]
fn mt549_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[39]);
}

#[test]
fn mt558_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[40]);
}

#[test]
fn mt564_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[41]);
}

#[test]
fn mt565_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[42]);
}

#[test]
fn mt566_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[43]);
}

#[test]
fn mt567_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[44]);
}

#[test]
fn mt568_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[45]);
}

#[test]
fn mt569_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[46]);
}

#[test]
fn mt575_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[47]);
}

#[test]
fn mt576_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[48]);
}

#[test]
fn mt578_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[49]);
}

#[test]
fn mt581_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[50]);
}

#[test]
fn mt586_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[51]);
}

#[test]
fn mt590_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[52]);
}

#[test]
fn mt591_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[53]);
}

#[test]
fn mt592_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[54]);
}

#[test]
fn mt595_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[55]);
}

#[test]
fn mt596_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[56]);
}

#[test]
fn mt598_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[57]);
}

#[test]
fn mt599_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[58]);
}

#[test]
fn mt670_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[59]);
}

#[test]
fn mt671_normalized_output_matches_golden() {
    assert_golden(&GOLDEN_CASES[60]);
}
