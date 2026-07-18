use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use swift_core::{parse_message, BlockId, FieldSlice, ParsedMessage, SequenceFrame};
use swift_schema::{
    match_and_parse_message, render_message, render_payload_column, CapturedFieldValue,
    FieldRuleSchema, RenderEnvelope, RenderRequest, RenderRow, SchemaCatalog,
};

struct RoundTripCase {
    message_type: &'static str,
    sample_file: &'static str,
}

const ROUNDTRIP_CASES: &[RoundTripCase] = &[
    RoundTripCase {
        message_type: "MT321",
        sample_file: "mt321_sample.fin",
    },
    RoundTripCase {
        message_type: "MT370",
        sample_file: "mt370_sample.fin",
    },
    RoundTripCase {
        message_type: "MT380",
        sample_file: "mt380_sample.fin",
    },
    RoundTripCase {
        message_type: "MT381",
        sample_file: "mt381_sample.fin",
    },
    RoundTripCase {
        message_type: "MT500",
        sample_file: "mt500_sample.fin",
    },
    RoundTripCase {
        message_type: "MT501",
        sample_file: "mt501_sample.fin",
    },
    RoundTripCase {
        message_type: "MT502",
        sample_file: "mt502_sample.fin",
    },
    RoundTripCase {
        message_type: "MT503",
        sample_file: "mt503_sample.fin",
    },
    RoundTripCase {
        message_type: "MT504",
        sample_file: "mt504_sample.fin",
    },
    RoundTripCase {
        message_type: "MT505",
        sample_file: "mt505_sample.fin",
    },
    RoundTripCase {
        message_type: "MT506",
        sample_file: "mt506_sample.fin",
    },
    RoundTripCase {
        message_type: "MT507",
        sample_file: "mt507_sample.fin",
    },
    RoundTripCase {
        message_type: "MT508",
        sample_file: "mt508_sample.fin",
    },
    RoundTripCase {
        message_type: "MT509",
        sample_file: "mt509_sample.fin",
    },
    RoundTripCase {
        message_type: "MT510",
        sample_file: "mt510_sample.fin",
    },
    RoundTripCase {
        message_type: "MT513",
        sample_file: "mt513_sample.fin",
    },
    RoundTripCase {
        message_type: "MT514",
        sample_file: "mt514_sample.fin",
    },
    RoundTripCase {
        message_type: "MT515",
        sample_file: "mt515_sample.fin",
    },
    RoundTripCase {
        message_type: "MT516",
        sample_file: "mt516_sample.fin",
    },
    RoundTripCase {
        message_type: "MT517",
        sample_file: "mt517_sample.fin",
    },
    RoundTripCase {
        message_type: "MT518",
        sample_file: "mt518_sample.fin",
    },
    RoundTripCase {
        message_type: "MT519",
        sample_file: "mt519_sample.fin",
    },
    RoundTripCase {
        message_type: "MT524",
        sample_file: "mt524_sample.fin",
    },
    RoundTripCase {
        message_type: "MT526",
        sample_file: "mt526_sample.fin",
    },
    RoundTripCase {
        message_type: "MT527",
        sample_file: "mt527_sample.fin",
    },
    RoundTripCase {
        message_type: "MT530",
        sample_file: "mt530_sample.fin",
    },
    RoundTripCase {
        message_type: "MT535",
        sample_file: "mt535_sample.fin",
    },
    RoundTripCase {
        message_type: "MT536",
        sample_file: "mt536_sample.fin",
    },
    RoundTripCase {
        message_type: "MT537",
        sample_file: "mt537_sample.fin",
    },
    RoundTripCase {
        message_type: "MT538",
        sample_file: "mt538_sample.fin",
    },
    RoundTripCase {
        message_type: "MT540",
        sample_file: "mt540_sample.fin",
    },
    RoundTripCase {
        message_type: "MT541",
        sample_file: "mt541_sample.fin",
    },
    RoundTripCase {
        message_type: "MT542",
        sample_file: "mt542_sample.fin",
    },
    RoundTripCase {
        message_type: "MT543",
        sample_file: "mt543_sample.fin",
    },
    RoundTripCase {
        message_type: "MT544",
        sample_file: "mt544_sample.fin",
    },
    RoundTripCase {
        message_type: "MT545",
        sample_file: "mt545_sample.fin",
    },
    RoundTripCase {
        message_type: "MT546",
        sample_file: "mt546_sample.fin",
    },
    RoundTripCase {
        message_type: "MT547",
        sample_file: "mt547_sample.fin",
    },
    RoundTripCase {
        message_type: "MT548",
        sample_file: "mt548_sample.fin",
    },
    RoundTripCase {
        message_type: "MT549",
        sample_file: "mt549_sample.fin",
    },
    RoundTripCase {
        message_type: "MT558",
        sample_file: "mt558_sample.fin",
    },
    RoundTripCase {
        message_type: "MT564",
        sample_file: "mt564_sample.fin",
    },
    RoundTripCase {
        message_type: "MT565",
        sample_file: "mt565_sample.fin",
    },
    RoundTripCase {
        message_type: "MT566",
        sample_file: "mt566_sample.fin",
    },
    RoundTripCase {
        message_type: "MT567",
        sample_file: "mt567_sample.fin",
    },
    RoundTripCase {
        message_type: "MT568",
        sample_file: "mt568_sample.fin",
    },
    RoundTripCase {
        message_type: "MT569",
        sample_file: "mt569_sample.fin",
    },
    RoundTripCase {
        message_type: "MT575",
        sample_file: "mt575_sample.fin",
    },
    RoundTripCase {
        message_type: "MT576",
        sample_file: "mt576_sample.fin",
    },
    RoundTripCase {
        message_type: "MT578",
        sample_file: "mt578_sample.fin",
    },
    RoundTripCase {
        message_type: "MT581",
        sample_file: "mt581_sample.fin",
    },
    RoundTripCase {
        message_type: "MT586",
        sample_file: "mt586_sample.fin",
    },
    RoundTripCase {
        message_type: "MT590",
        sample_file: "mt590_sample.fin",
    },
    RoundTripCase {
        message_type: "MT591",
        sample_file: "mt591_sample.fin",
    },
    RoundTripCase {
        message_type: "MT592",
        sample_file: "mt592_sample.fin",
    },
    RoundTripCase {
        message_type: "MT595",
        sample_file: "mt595_sample.fin",
    },
    RoundTripCase {
        message_type: "MT596",
        sample_file: "mt596_sample.fin",
    },
    RoundTripCase {
        message_type: "MT598",
        sample_file: "mt598_sample.fin",
    },
    RoundTripCase {
        message_type: "MT599",
        sample_file: "mt599_sample.fin",
    },
    RoundTripCase {
        message_type: "MT670",
        sample_file: "mt670_sample.fin",
    },
    RoundTripCase {
        message_type: "MT671",
        sample_file: "mt671_sample.fin",
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

fn envelope(parsed: &ParsedMessage<'_>) -> RenderEnvelope {
    RenderEnvelope {
        block1: block_content(parsed, BlockId::BasicHeader),
        block2: block_content(parsed, BlockId::ApplicationHeader),
        block3: optional_block_content(parsed, BlockId::UserHeader),
        block5: optional_block_content(parsed, BlockId::Trailer),
    }
}

fn block_content(parsed: &ParsedMessage<'_>, id: BlockId<'_>) -> String {
    optional_block_content(parsed, id).expect("sample should include required envelope block")
}

fn optional_block_content(parsed: &ParsedMessage<'_>, id: BlockId<'_>) -> Option<String> {
    parsed
        .blocks
        .iter()
        .find(|block| block.id == id)
        .map(|block| String::from_utf8_lossy(block.content).into_owned())
}

fn render_rows(
    catalog: &SchemaCatalog,
    message_type: &str,
    message_id: &str,
    parsed: &ParsedMessage<'_>,
) -> Vec<RenderRow> {
    let schema = catalog
        .message(message_type)
        .expect("message schema should exist");
    let parsed_match = match_and_parse_message(catalog, schema, parsed);
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

    let mut rows = Vec::new();
    for matched in &parsed_match.matched_fields {
        merge_render_row(
            &mut rows,
            render_row(
                message_id,
                matched.field.sequence_path.as_slice(),
                matched.rule,
                matched.field_type,
                &matched.captures,
            ),
        );
    }
    rows
}

fn merge_render_row(rows: &mut Vec<RenderRow>, row: RenderRow) {
    let message_id = row.values.get("message_id");
    let sequence_path = row.values.get("sequence_path");
    if let Some(existing) = rows.iter_mut().find(|existing| {
        existing.table == row.table
            && existing.values.get("message_id") == message_id
            && existing.values.get("sequence_path") == sequence_path
    }) {
        existing.values.extend(row.values);
    } else {
        rows.push(row);
    }
}

fn render_row(
    message_id: &str,
    sequence_path: &[SequenceFrame<'_>],
    rule: &FieldRuleSchema,
    field_type: &str,
    captures: &[CapturedFieldValue<'_>],
) -> RenderRow {
    let mut values = BTreeMap::new();
    values.insert("message_id".to_string(), message_id.to_string());
    values.insert(
        "sequence_path".to_string(),
        sequence_path_to_string(sequence_path).unwrap_or_else(|| "$".to_string()),
    );
    let capture_value = selected_capture_value(captures);
    values.insert(render_payload_column(rule), capture_value.clone());
    values.insert(
        rule.column.clone(),
        normalize_capture_value(field_type, capture_value),
    );

    RenderRow {
        table: rule.entity.clone(),
        values,
    }
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

fn selected_capture_value(captures: &[CapturedFieldValue<'_>]) -> String {
    for preferred in ["value", "date", "amount", "quantity", "code"] {
        if let Some(capture) = captures.iter().find(|capture| capture.name == preferred) {
            return String::from_utf8_lossy(capture.value).into_owned();
        }
    }

    captures
        .last()
        .map(|capture| String::from_utf8_lossy(capture.value).into_owned())
        .unwrap_or_default()
}

fn sequence_path_to_string(path: &[SequenceFrame<'_>]) -> Option<String> {
    if path.is_empty() {
        return None;
    }

    Some(
        path.iter()
            .map(|frame| {
                format!(
                    "{}[{}]",
                    String::from_utf8_lossy(frame.name),
                    frame.occurrence
                )
            })
            .collect::<Vec<_>>()
            .join("/"),
    )
}

fn structural_fields(parsed: &ParsedMessage<'_>) -> Vec<(String, String, Option<String>, String)> {
    let mut fields = parsed
        .fields
        .iter()
        .map(structural_field)
        .collect::<Vec<_>>();
    fields.sort();
    fields
}

fn structural_field(field: &FieldSlice<'_>) -> (String, String, Option<String>, String) {
    (
        sequence_path_to_string(&field.sequence_path).unwrap_or_else(|| "$".to_string()),
        String::from_utf8_lossy(field.tag).into_owned(),
        field
            .qualifier
            .map(|qualifier| String::from_utf8_lossy(qualifier).into_owned()),
        String::from_utf8_lossy(field.value).into_owned(),
    )
}

fn assert_roundtrip(case: &RoundTripCase) {
    let catalog = load_catalog();
    let input = load_sample(case.sample_file);
    let parsed = parse_message(&input);
    assert!(
        parsed.diagnostics.is_empty(),
        "sample should parse cleanly: {:?}",
        parsed.diagnostics
    );

    let rendered = render_message(
        &catalog,
        &RenderRequest {
            message_id: case.message_type.to_ascii_lowercase(),
            message_type: case.message_type.to_string(),
            envelope: envelope(&parsed),
            rows: render_rows(
                &catalog,
                case.message_type,
                &case.message_type.to_ascii_lowercase(),
                &parsed,
            ),
        },
    )
    .expect("sample should render");
    let reparsed = parse_message(rendered.as_bytes());
    assert!(
        reparsed.diagnostics.is_empty(),
        "rendered sample should parse cleanly: {:?}",
        reparsed.diagnostics
    );

    // Rendering canonicalizes SWIFT field text and can reorder equivalent
    // schema fields, so compare the structural field-slice set: sequence path,
    // tag, qualifier, and field value.
    assert_eq!(structural_fields(&parsed), structural_fields(&reparsed));
}

#[test]
fn mt321_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[0]);
}

#[test]
fn mt370_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[1]);
}

#[test]
fn mt380_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[2]);
}

#[test]
fn mt381_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[3]);
}

#[test]
fn mt500_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[4]);
}

#[test]
fn mt501_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[5]);
}

#[test]
fn mt502_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[6]);
}

#[test]
fn mt503_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[7]);
}

#[test]
fn mt504_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[8]);
}

#[test]
fn mt505_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[9]);
}

#[test]
fn mt506_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[10]);
}

#[test]
fn mt507_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[11]);
}

#[test]
fn mt508_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[12]);
}

#[test]
fn mt509_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[13]);
}

#[test]
fn mt510_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[14]);
}

#[test]
fn mt513_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[15]);
}

#[test]
fn mt514_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[16]);
}

#[test]
fn mt515_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[17]);
}

#[test]
fn mt516_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[18]);
}

#[test]
fn mt517_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[19]);
}

#[test]
fn mt518_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[20]);
}

#[test]
fn mt519_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[21]);
}

#[test]
fn mt524_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[22]);
}

#[test]
fn mt526_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[23]);
}

#[test]
fn mt527_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[24]);
}

#[test]
fn mt530_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[25]);
}

#[test]
fn mt535_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[26]);
}

#[test]
fn mt536_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[27]);
}

#[test]
fn mt537_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[28]);
}

#[test]
fn mt538_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[29]);
}

#[test]
fn mt540_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[30]);
}

#[test]
fn mt541_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[31]);
}

#[test]
fn mt542_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[32]);
}

#[test]
fn mt543_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[33]);
}

#[test]
fn mt544_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[34]);
}

#[test]
fn mt545_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[35]);
}

#[test]
fn mt546_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[36]);
}

#[test]
fn mt547_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[37]);
}

#[test]
fn mt548_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[38]);
}

#[test]
fn mt549_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[39]);
}

#[test]
fn mt558_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[40]);
}

#[test]
fn mt564_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[41]);
}

#[test]
fn mt565_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[42]);
}

#[test]
fn mt566_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[43]);
}

#[test]
fn mt567_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[44]);
}

#[test]
fn mt568_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[45]);
}

#[test]
fn mt569_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[46]);
}

#[test]
fn mt575_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[47]);
}

#[test]
fn mt576_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[48]);
}

#[test]
fn mt578_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[49]);
}

#[test]
fn mt581_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[50]);
}

#[test]
fn mt586_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[51]);
}

#[test]
fn mt590_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[52]);
}

#[test]
fn mt591_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[53]);
}

#[test]
fn mt592_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[54]);
}

#[test]
fn mt595_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[55]);
}

#[test]
fn mt596_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[56]);
}

#[test]
fn mt598_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[57]);
}

#[test]
fn mt599_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[58]);
}

#[test]
fn mt670_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[59]);
}

#[test]
fn mt671_parse_render_reparse_roundtrip_is_structurally_stable() {
    assert_roundtrip(&ROUNDTRIP_CASES[60]);
}
