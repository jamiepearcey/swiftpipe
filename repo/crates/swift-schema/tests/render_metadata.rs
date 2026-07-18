use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use swift_core::parse_message;
use swift_schema::{
    match_and_parse_message, FieldPatternStep, FieldRuleSchema, ParsedMatchedField, SchemaCatalog,
};

struct RenderMetadataCase {
    message_type: &'static str,
    schema_file: &'static str,
    sample_file: &'static str,
}

const RENDER_METADATA_CASES: &[RenderMetadataCase] = &[
    RenderMetadataCase {
        message_type: "MT321",
        schema_file: "mt321.yaml",
        sample_file: "mt321_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT370",
        schema_file: "mt370.yaml",
        sample_file: "mt370_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT380",
        schema_file: "mt380.yaml",
        sample_file: "mt380_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT381",
        schema_file: "mt381.yaml",
        sample_file: "mt381_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT500",
        schema_file: "mt500.yaml",
        sample_file: "mt500_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT501",
        schema_file: "mt501.yaml",
        sample_file: "mt501_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT502",
        schema_file: "mt502.yaml",
        sample_file: "mt502_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT503",
        schema_file: "mt503.yaml",
        sample_file: "mt503_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT504",
        schema_file: "mt504.yaml",
        sample_file: "mt504_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT505",
        schema_file: "mt505.yaml",
        sample_file: "mt505_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT506",
        schema_file: "mt506.yaml",
        sample_file: "mt506_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT507",
        schema_file: "mt507.yaml",
        sample_file: "mt507_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT508",
        schema_file: "mt508.yaml",
        sample_file: "mt508_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT509",
        schema_file: "mt509.yaml",
        sample_file: "mt509_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT510",
        schema_file: "mt510.yaml",
        sample_file: "mt510_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT513",
        schema_file: "mt513.yaml",
        sample_file: "mt513_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT514",
        schema_file: "mt514.yaml",
        sample_file: "mt514_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT515",
        schema_file: "mt515.yaml",
        sample_file: "mt515_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT516",
        schema_file: "mt516.yaml",
        sample_file: "mt516_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT517",
        schema_file: "mt517.yaml",
        sample_file: "mt517_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT518",
        schema_file: "mt518.yaml",
        sample_file: "mt518_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT519",
        schema_file: "mt519.yaml",
        sample_file: "mt519_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT524",
        schema_file: "mt524.yaml",
        sample_file: "mt524_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT526",
        schema_file: "mt526.yaml",
        sample_file: "mt526_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT527",
        schema_file: "mt527.yaml",
        sample_file: "mt527_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT530",
        schema_file: "mt530.yaml",
        sample_file: "mt530_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT535",
        schema_file: "mt535.yaml",
        sample_file: "mt535_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT536",
        schema_file: "mt536.yaml",
        sample_file: "mt536_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT537",
        schema_file: "mt537.yaml",
        sample_file: "mt537_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT538",
        schema_file: "mt538.yaml",
        sample_file: "mt538_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT540",
        schema_file: "mt540.yaml",
        sample_file: "mt540_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT541",
        schema_file: "mt541.yaml",
        sample_file: "mt541_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT542",
        schema_file: "mt542.yaml",
        sample_file: "mt542_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT543",
        schema_file: "mt543.yaml",
        sample_file: "mt543_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT544",
        schema_file: "mt544.yaml",
        sample_file: "mt544_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT545",
        schema_file: "mt545.yaml",
        sample_file: "mt545_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT546",
        schema_file: "mt546.yaml",
        sample_file: "mt546_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT547",
        schema_file: "mt547.yaml",
        sample_file: "mt547_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT548",
        schema_file: "mt548.yaml",
        sample_file: "mt548_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT549",
        schema_file: "mt549.yaml",
        sample_file: "mt549_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT558",
        schema_file: "mt558.yaml",
        sample_file: "mt558_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT564",
        schema_file: "mt564.yaml",
        sample_file: "mt564_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT565",
        schema_file: "mt565.yaml",
        sample_file: "mt565_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT566",
        schema_file: "mt566.yaml",
        sample_file: "mt566_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT567",
        schema_file: "mt567.yaml",
        sample_file: "mt567_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT568",
        schema_file: "mt568.yaml",
        sample_file: "mt568_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT569",
        schema_file: "mt569.yaml",
        sample_file: "mt569_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT575",
        schema_file: "mt575.yaml",
        sample_file: "mt575_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT576",
        schema_file: "mt576.yaml",
        sample_file: "mt576_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT578",
        schema_file: "mt578.yaml",
        sample_file: "mt578_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT581",
        schema_file: "mt581.yaml",
        sample_file: "mt581_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT586",
        schema_file: "mt586.yaml",
        sample_file: "mt586_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT590",
        schema_file: "mt590.yaml",
        sample_file: "mt590_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT591",
        schema_file: "mt591.yaml",
        sample_file: "mt591_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT592",
        schema_file: "mt592.yaml",
        sample_file: "mt592_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT595",
        schema_file: "mt595.yaml",
        sample_file: "mt595_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT596",
        schema_file: "mt596.yaml",
        sample_file: "mt596_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT598",
        schema_file: "mt598.yaml",
        sample_file: "mt598_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT599",
        schema_file: "mt599.yaml",
        sample_file: "mt599_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT670",
        schema_file: "mt670.yaml",
        sample_file: "mt670_sample.fin",
    },
    RenderMetadataCase {
        message_type: "MT671",
        schema_file: "mt671.yaml",
        sample_file: "mt671_sample.fin",
    },
];

#[test]
fn mt321_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[0]);
}

#[test]
fn mt370_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[1]);
}

#[test]
fn mt380_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[2]);
}

#[test]
fn mt381_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[3]);
}

#[test]
fn mt500_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[4]);
}

#[test]
fn mt501_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[5]);
}

#[test]
fn mt502_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[6]);
}

#[test]
fn mt503_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[7]);
}

#[test]
fn mt504_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[8]);
}

#[test]
fn mt505_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[9]);
}

#[test]
fn mt506_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[10]);
}

#[test]
fn mt507_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[11]);
}

#[test]
fn mt508_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[12]);
}

#[test]
fn mt509_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[13]);
}

#[test]
fn mt510_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[14]);
}

#[test]
fn mt513_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[15]);
}

#[test]
fn mt514_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[16]);
}

#[test]
fn mt515_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[17]);
}

#[test]
fn mt516_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[18]);
}

#[test]
fn mt517_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[19]);
}

#[test]
fn mt518_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[20]);
}

#[test]
fn mt519_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[21]);
}

#[test]
fn mt524_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[22]);
}

#[test]
fn mt526_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[23]);
}

#[test]
fn mt527_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[24]);
}

#[test]
fn mt530_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[25]);
}

#[test]
fn mt535_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[26]);
}

#[test]
fn mt536_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[27]);
}

#[test]
fn mt537_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[28]);
}

#[test]
fn mt538_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[29]);
}

#[test]
fn mt540_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[30]);
}

#[test]
fn mt541_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[31]);
}

#[test]
fn mt542_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[32]);
}

#[test]
fn mt543_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[33]);
}

#[test]
fn mt544_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[34]);
}

#[test]
fn mt545_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[35]);
}

#[test]
fn mt546_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[36]);
}

#[test]
fn mt547_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[37]);
}

#[test]
fn mt548_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[38]);
}

#[test]
fn mt549_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[39]);
}

#[test]
fn mt558_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[40]);
}

#[test]
fn mt564_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[41]);
}

#[test]
fn mt565_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[42]);
}

#[test]
fn mt566_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[43]);
}

#[test]
fn mt567_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[44]);
}

#[test]
fn mt568_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[45]);
}

#[test]
fn mt569_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[46]);
}

#[test]
fn mt575_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[47]);
}

#[test]
fn mt576_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[48]);
}

#[test]
fn mt578_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[49]);
}

#[test]
fn mt581_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[50]);
}

#[test]
fn mt586_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[51]);
}

#[test]
fn mt590_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[52]);
}

#[test]
fn mt591_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[53]);
}

#[test]
fn mt592_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[54]);
}

#[test]
fn mt595_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[55]);
}

#[test]
fn mt596_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[56]);
}

#[test]
fn mt598_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[57]);
}

#[test]
fn mt599_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[58]);
}

#[test]
fn mt670_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[59]);
}

#[test]
fn mt671_sample_fields_have_unambiguous_render_metadata() {
    assert_render_metadata(&RENDER_METADATA_CASES[60]);
}

fn assert_render_metadata(case: &RenderMetadataCase) {
    let catalog = load_catalog(case.schema_file);
    let message = catalog
        .message(case.message_type)
        .expect("message schema should exist");
    let sample = fs::read(repo_root().join("examples").join(case.sample_file))
        .expect("sample should be readable");
    let parsed = parse_message(&sample);
    let message_match = match_and_parse_message(&catalog, message, &parsed);

    assert!(
        message_match.parse_errors.is_empty(),
        "{} sample fields should parse without errors: {:#?}",
        case.message_type,
        message_match.parse_errors
    );

    let ambiguous_render_options = matched_rule_names(
        message_match
            .matched_fields
            .iter()
            .filter(|matched| render_option_is_ambiguous(matched))
            .map(|matched| matched.rule),
    );
    let missing_render_qualifiers = matched_rule_names(
        message_match
            .matched_fields
            .iter()
            .filter(|matched| render_qualifier_is_missing(&catalog, matched))
            .map(|matched| matched.rule),
    );

    assert!(
        ambiguous_render_options.is_empty() && missing_render_qualifiers.is_empty(),
        "{} render metadata coverage failed:\nambiguous_render_options: {:#?}\nmissing_render_qualifiers: {:#?}",
        case.message_type,
        ambiguous_render_options,
        missing_render_qualifiers
    );
}

fn repo_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("crate should live under the workspace crates directory")
        .to_path_buf()
}

fn load_catalog(schema_file: &str) -> SchemaCatalog {
    let source = fs::read_to_string(repo_root().join("examples/schemas").join(schema_file))
        .expect("schema should be readable");
    let catalog = SchemaCatalog::from_yaml_str(&source).expect("schema should parse");
    catalog
        .validate_rendering()
        .expect("schema should validate for rendering");
    catalog
}

fn render_option_is_ambiguous(matched: &ParsedMatchedField<'_, '_, '_>) -> bool {
    matched.rule.options.len() > 1
        && matched
            .rule
            .render
            .as_ref()
            .and_then(|render| render.option.as_ref())
            .is_none()
}

fn render_qualifier_is_missing(
    catalog: &SchemaCatalog,
    matched: &ParsedMatchedField<'_, '_, '_>,
) -> bool {
    matched.rule.qualifier.is_none()
        && matched
            .rule
            .render
            .as_ref()
            .and_then(|render| render.qualifier.as_ref())
            .is_none()
        && catalog
            .field_type(matched.field_type)
            .is_some_and(|field_type| field_type_uses_capture(field_type, "qualifier"))
}

fn field_type_uses_capture(field_type: &swift_schema::FieldTypeSchema, capture_name: &str) -> bool {
    field_type.pattern.iter().any(|step| match step {
        FieldPatternStep::Capture { name, .. } | FieldPatternStep::Rest { name } => {
            name == capture_name
        }
        FieldPatternStep::Literal { .. } => false,
    })
}

fn matched_rule_names<'schema>(
    rules: impl Iterator<Item = &'schema FieldRuleSchema>,
) -> Vec<String> {
    rules
        .map(|rule| {
            format!(
                "{} {} {}{}",
                rule.path,
                rule.tag,
                rule.name,
                rule.qualifier
                    .as_ref()
                    .map_or(String::new(), |qualifier| format!(" ({qualifier})"))
            )
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
