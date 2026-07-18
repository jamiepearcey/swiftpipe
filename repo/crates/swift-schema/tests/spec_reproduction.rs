use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use swift_core::parse_message;
use swift_schema::{match_and_parse_message, SchemaCatalog};

struct SpecCase {
    message: &'static str,
    schema_file: &'static str,
    sample_file: &'static str,
    must_match_fields: &'static [&'static str],
    min_matches: usize,
}

const SPECS: &[SpecCase] = &[
    SpecCase {
        message: "MT321",
        schema_file: "examples/schemas/mt321.yaml",
        sample_file: "examples/mt321_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT370",
        schema_file: "examples/schemas/mt370.yaml",
        sample_file: "examples/mt370_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT380",
        schema_file: "examples/schemas/mt380.yaml",
        sample_file: "examples/mt380_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT381",
        schema_file: "examples/schemas/mt381.yaml",
        sample_file: "examples/mt381_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT500",
        schema_file: "examples/schemas/mt500.yaml",
        sample_file: "examples/mt500_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT501",
        schema_file: "examples/schemas/mt501.yaml",
        sample_file: "examples/mt501_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT502",
        schema_file: "examples/schemas/mt502.yaml",
        sample_file: "examples/mt502_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT503",
        schema_file: "examples/schemas/mt503.yaml",
        sample_file: "examples/mt503_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT504",
        schema_file: "examples/schemas/mt504.yaml",
        sample_file: "examples/mt504_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT505",
        schema_file: "examples/schemas/mt505.yaml",
        sample_file: "examples/mt505_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT506",
        schema_file: "examples/schemas/mt506.yaml",
        sample_file: "examples/mt506_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT507",
        schema_file: "examples/schemas/mt507.yaml",
        sample_file: "examples/mt507_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT508",
        schema_file: "examples/schemas/mt508.yaml",
        sample_file: "examples/mt508_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT509",
        schema_file: "examples/schemas/mt509.yaml",
        sample_file: "examples/mt509_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT510",
        schema_file: "examples/schemas/mt510.yaml",
        sample_file: "examples/mt510_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT513",
        schema_file: "examples/schemas/mt513.yaml",
        sample_file: "examples/mt513_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT514",
        schema_file: "examples/schemas/mt514.yaml",
        sample_file: "examples/mt514_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT515",
        schema_file: "examples/schemas/mt515.yaml",
        sample_file: "examples/mt515_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT516",
        schema_file: "examples/schemas/mt516.yaml",
        sample_file: "examples/mt516_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT517",
        schema_file: "examples/schemas/mt517.yaml",
        sample_file: "examples/mt517_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT518",
        schema_file: "examples/schemas/mt518.yaml",
        sample_file: "examples/mt518_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT519",
        schema_file: "examples/schemas/mt519.yaml",
        sample_file: "examples/mt519_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT524",
        schema_file: "examples/schemas/mt524.yaml",
        sample_file: "examples/mt524_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT526",
        schema_file: "examples/schemas/mt526.yaml",
        sample_file: "examples/mt526_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT527",
        schema_file: "examples/schemas/mt527.yaml",
        sample_file: "examples/mt527_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT530",
        schema_file: "examples/schemas/mt530.yaml",
        sample_file: "examples/mt530_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT535",
        schema_file: "examples/schemas/mt535.yaml",
        sample_file: "examples/mt535_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT536",
        schema_file: "examples/schemas/mt536.yaml",
        sample_file: "examples/mt536_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT537",
        schema_file: "examples/schemas/mt537.yaml",
        sample_file: "examples/mt537_sample.fin",
        must_match_fields: &["sender_reference", "function", "statement_date"],
        min_matches: 10,
    },
    SpecCase {
        message: "MT538",
        schema_file: "examples/schemas/mt538.yaml",
        sample_file: "examples/mt538_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT540",
        schema_file: "examples/schemas/mt540.yaml",
        sample_file: "examples/mt540_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT541",
        schema_file: "examples/schemas/mt541.yaml",
        sample_file: "examples/mt541_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_amount"],
        min_matches: 7,
    },
    SpecCase {
        message: "MT542",
        schema_file: "examples/schemas/mt542.yaml",
        sample_file: "examples/mt542_sample.fin",
        must_match_fields: &[
            "sender_reference",
            "preparation_date",
            "settlement_quantity",
        ],
        min_matches: 6,
    },
    SpecCase {
        message: "MT543",
        schema_file: "examples/schemas/mt543.yaml",
        sample_file: "examples/mt543_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_amount"],
        min_matches: 7,
    },
    SpecCase {
        message: "MT544",
        schema_file: "examples/schemas/mt544.yaml",
        sample_file: "examples/mt544_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT545",
        schema_file: "examples/schemas/mt545.yaml",
        sample_file: "examples/mt545_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT546",
        schema_file: "examples/schemas/mt546.yaml",
        sample_file: "examples/mt546_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT547",
        schema_file: "examples/schemas/mt547.yaml",
        sample_file: "examples/mt547_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT548",
        schema_file: "examples/schemas/mt548.yaml",
        sample_file: "examples/mt548_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT549",
        schema_file: "examples/schemas/mt549.yaml",
        sample_file: "examples/mt549_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT558",
        schema_file: "examples/schemas/mt558.yaml",
        sample_file: "examples/mt558_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT564",
        schema_file: "examples/schemas/mt564.yaml",
        sample_file: "examples/mt564_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT565",
        schema_file: "examples/schemas/mt565.yaml",
        sample_file: "examples/mt565_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT566",
        schema_file: "examples/schemas/mt566.yaml",
        sample_file: "examples/mt566_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT567",
        schema_file: "examples/schemas/mt567.yaml",
        sample_file: "examples/mt567_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT568",
        schema_file: "examples/schemas/mt568.yaml",
        sample_file: "examples/mt568_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT569",
        schema_file: "examples/schemas/mt569.yaml",
        sample_file: "examples/mt569_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT575",
        schema_file: "examples/schemas/mt575.yaml",
        sample_file: "examples/mt575_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT576",
        schema_file: "examples/schemas/mt576.yaml",
        sample_file: "examples/mt576_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT578",
        schema_file: "examples/schemas/mt578.yaml",
        sample_file: "examples/mt578_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT581",
        schema_file: "examples/schemas/mt581.yaml",
        sample_file: "examples/mt581_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT586",
        schema_file: "examples/schemas/mt586.yaml",
        sample_file: "examples/mt586_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT590",
        schema_file: "examples/schemas/mt590.yaml",
        sample_file: "examples/mt590_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT591",
        schema_file: "examples/schemas/mt591.yaml",
        sample_file: "examples/mt591_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT592",
        schema_file: "examples/schemas/mt592.yaml",
        sample_file: "examples/mt592_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT595",
        schema_file: "examples/schemas/mt595.yaml",
        sample_file: "examples/mt595_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT596",
        schema_file: "examples/schemas/mt596.yaml",
        sample_file: "examples/mt596_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT598",
        schema_file: "examples/schemas/mt598.yaml",
        sample_file: "examples/mt598_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT599",
        schema_file: "examples/schemas/mt599.yaml",
        sample_file: "examples/mt599_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT670",
        schema_file: "examples/schemas/mt670.yaml",
        sample_file: "examples/mt670_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
    SpecCase {
        message: "MT671",
        schema_file: "examples/schemas/mt671.yaml",
        sample_file: "examples/mt671_sample.fin",
        must_match_fields: &["sender_reference", "preparation_date", "settlement_date"],
        min_matches: 6,
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReproCandidate {
    message: String,
    schema_file: String,
    sample_file: String,
}

fn examples_dir() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .to_path_buf()
}

fn repo_file(path: &str) -> PathBuf {
    examples_dir().join(path)
}

fn load_schema(path: &str) -> SchemaCatalog {
    let full_path = repo_file(path);
    let source = fs::read_to_string(full_path).expect("schema should be readable");
    SchemaCatalog::from_yaml_str(&source).expect("schema should parse")
}

fn load_message(path: &str) -> Vec<u8> {
    let full_path = repo_file(path);
    fs::read(full_path).expect("sample should be readable")
}

fn discover_reproducible_cases() -> Vec<ReproCandidate> {
    let examples_root = examples_dir();
    let schema_dir = examples_root.join("examples/schemas");
    let mut case_by_message = BTreeMap::<String, String>::new();

    let schema_entries = fs::read_dir(&schema_dir).expect("schemas directory should be readable");
    for entry in schema_entries {
        let entry = entry.expect("schema entry should be readable");
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("yaml") {
            continue;
        }

        let source = fs::read_to_string(&path).expect("schema file should be readable");
        let catalog = SchemaCatalog::from_yaml_str(&source)
            .expect("schema should parse when discovering reproducible cases");

        for message in catalog.messages {
            let schema_file = path
                .strip_prefix(&examples_root)
                .expect("schema path must be under examples")
                .to_string_lossy()
                .into_owned();
            case_by_message.insert(message.message, schema_file);
        }
    }

    let mut cases = Vec::new();
    let sample_dir = examples_root.join("examples");
    let sample_entries = fs::read_dir(&sample_dir).expect("examples dir should be readable");
    for entry in sample_entries {
        let entry = entry.expect("sample entry should be readable");
        let name = entry.file_name().to_string_lossy().to_string();

        if let Some(message) = name
            .strip_prefix("mt")
            .and_then(|suffix| suffix.strip_suffix("_sample.fin"))
        {
            if message.chars().all(|c| c.is_ascii_digit()) {
                let mt = format!("MT{message}");
                if let Some(schema_file) = case_by_message.get(&mt).cloned() {
                    cases.push(ReproCandidate {
                        message: mt,
                        schema_file,
                        sample_file: format!("examples/{name}"),
                    });
                }
            }
        }
    }

    cases.sort_by(|a, b| a.message.cmp(&b.message));
    cases
}

fn parse_uhb_file(path: &Path) -> (usize, usize, bool) {
    let content = fs::read_to_string(path).expect("cached UHB spec should be readable");

    let mut in_format_section = false;
    let mut field_rows = 0;
    let mut tag_rows = 0;
    let mut has_known_header = false;

    for raw_line in content.lines() {
        let line = raw_line.trim();

        if line.contains("| Status | Tag |") {
            has_known_header = true;
            in_format_section = true;
            continue;
        }

        if !in_format_section {
            continue;
        }

        if line.starts_with("## MT") && line.contains("Network Validated Rules") {
            break;
        }

        if !line.starts_with('|') {
            continue;
        }

        let cells: Vec<&str> = line
            .trim_start_matches('|')
            .trim_end_matches('|')
            .split('|')
            .map(|value| value.trim())
            .collect();

        if cells.is_empty() || cells[0].is_empty() {
            continue;
        }

        if cells[0].starts_with("---") {
            continue;
        }

        if cells[0] == "M" || cells[0] == "O" || cells[0] == "C" {
            field_rows += 1;
        }

        if cells.len() > 1 && !cells[0].is_empty() && !cells[1].is_empty() {
            tag_rows += 1;
        }
    }

    (field_rows, tag_rows, has_known_header)
}

fn common_group_file_for(path: &Path) -> Option<PathBuf> {
    let content = fs::read_to_string(path).expect("cached UHB spec should be readable");
    let parent = path.parent()?;
    for line in content.lines() {
        let mut search_offset = 0;
        while let Some(found) = line[search_offset..].find("finmtn") {
            let snippet = &line[search_offset + found + 6..];
            let group = snippet
                .chars()
                .take_while(|ch| ch.is_ascii_digit())
                .take(2)
                .collect::<String>();

            if group.len() == 2 {
                return Some(parent.join(format!("finmtn{group}.md")));
            }

            search_offset += found + 6 + snippet.len();
        }
    }

    None
}

fn parse_uhb_file_with_fallback(path: &Path) -> (usize, usize, bool) {
    let direct = parse_uhb_file(path);
    if direct.2 || direct.0 > 0 || direct.1 > 0 {
        return direct;
    }

    match common_group_file_for(path) {
        Some(group_path) if group_path.exists() => parse_uhb_file(&group_path),
        _ => direct,
    }
}

fn assert_spec_reproduced(case: &SpecCase) {
    let catalog = load_schema(case.schema_file);
    catalog.validate().expect("schema should validate");
    let schema = catalog
        .message(case.message)
        .expect("message schema should exist in catalog");

    let sample = load_message(case.sample_file);
    let parsed = parse_message(&sample);

    assert!(
        parsed.diagnostics.is_empty(),
        "structural parse diagnostics should be empty for {}",
        case.message
    );

    let matched = match_and_parse_message(&catalog, schema, &parsed);
    let matched_names: BTreeSet<&str> = matched
        .matched_fields
        .iter()
        .map(|field| field.rule.name.as_str())
        .collect();

    assert!(
        matched.parse_errors.is_empty(),
        "field parsing errors for {}: {:?}",
        case.message,
        matched.parse_errors
    );
    assert!(
        matched.missing_required.is_empty(),
        "missing required fields for {}: {:?}",
        case.message,
        matched.missing_required
    );
    assert!(
        matched.cardinality_violations.is_empty(),
        "cardinality issues for {}: {:?}",
        case.message,
        matched.cardinality_violations
    );
    assert!(
        matched.sequence_issues.is_empty(),
        "sequence issues for {}: {:?}",
        case.message,
        matched.sequence_issues
    );
    assert!(
        matched.matched_fields.len() >= case.min_matches,
        "expected at least {} matched fields for {}",
        case.min_matches,
        case.message
    );

    for field in case.must_match_fields {
        assert!(
            matched_names.contains(field),
            "expected matched field '{}' in {}",
            field,
            case.message
        );
    }
}

fn assert_repro_candidate(candidate: &ReproCandidate, should_apply_quality: bool) {
    let catalog = {
        let full_path = examples_dir().join(&candidate.schema_file);
        let source = fs::read_to_string(full_path).expect("schema should be readable");
        SchemaCatalog::from_yaml_str(&source).expect("schema should parse")
    };
    catalog.validate().expect("schema should validate");
    let schema = catalog
        .message(&candidate.message)
        .expect("message schema should exist in catalog");

    let sample = {
        let sample_path = examples_dir().join(&candidate.sample_file);
        fs::read(sample_path).expect("sample should be readable")
    };
    let parsed = parse_message(&sample);

    assert!(
        parsed.diagnostics.is_empty(),
        "structural parse diagnostics should be empty for {}",
        candidate.message
    );

    let matched = match_and_parse_message(&catalog, schema, &parsed);
    assert!(
        matched.parse_errors.is_empty(),
        "field parsing errors for {}: {:?}",
        candidate.message,
        matched.parse_errors
    );
    assert!(
        matched.missing_required.is_empty(),
        "missing required fields for {}: {:?}",
        candidate.message,
        matched.missing_required
    );
    assert!(
        matched.cardinality_violations.is_empty(),
        "cardinality issues for {}: {:?}",
        candidate.message,
        matched.cardinality_violations
    );
    assert!(
        matched.sequence_issues.is_empty(),
        "sequence issues for {}: {:?}",
        candidate.message,
        matched.sequence_issues
    );
    assert!(
        !matched.matched_fields.is_empty(),
        "expected some matched fields for {}",
        candidate.message
    );

    if should_apply_quality {
        let Some(constraint) = SPECS.iter().find(|spec| spec.message == candidate.message) else {
            return;
        };

        let matched_names: BTreeSet<&str> = matched
            .matched_fields
            .iter()
            .map(|field| field.rule.name.as_str())
            .collect();

        assert!(
            matched.matched_fields.len() >= constraint.min_matches,
            "expected at least {} matched fields for {}",
            constraint.min_matches,
            candidate.message
        );

        for field in constraint.must_match_fields {
            assert!(
                matched_names.contains(field),
                "expected matched field '{}' in {}",
                field,
                candidate.message
            );
        }
    }
}

#[test]
fn reproduces_declared_spec_cases() {
    for case in SPECS {
        assert_spec_reproduced(case);
    }
}

#[test]
fn reproduces_single_declared_spec_case() {
    let Some(message) = std::env::var("SPEC_REPRO_CASE").ok() else {
        return;
    };

    let Some(case) = SPECS.iter().find(|spec| spec.message == message.as_str()) else {
        panic!(
            "SPEC_REPRO_CASE '{}' is not declared in this test file",
            message
        );
    };

    assert_spec_reproduced(case);
}

#[test]
fn reproduces_all_discovered_local_cases_with_assets() {
    for candidate in discover_reproducible_cases() {
        let should_apply_quality = SPECS.iter().any(|spec| spec.message == candidate.message);
        assert_repro_candidate(&candidate, should_apply_quality);
    }
}

#[test]
fn every_available_local_repro_case_is_declared_in_known_cases() {
    let known_messages: BTreeSet<&str> = SPECS.iter().map(|spec| spec.message).collect();
    let discovered = discover_reproducible_cases();

    for candidate in discovered {
        assert!(
            known_messages.contains(candidate.message.as_str()),
            "local reproducible case {} is missing a declared SpecCase entry",
            candidate.message
        );
    }
}

#[test]
fn all_local_repro_cases_have_parseable_uhb_spec_tables() {
    let cache_dir = examples_dir().join("examples/.uhb");

    for candidate in discover_reproducible_cases() {
        let mt = candidate.message.trim_start_matches("MT");

        let path = cache_dir.join(format!("finmt{mt}.md"));
        assert!(
            path.exists(),
            "MT{} expected to have cached UHB spec file {}",
            mt,
            path.display()
        );

        let (fields, tags, has_known_header) = parse_uhb_file_with_fallback(&path);
        assert!(
            has_known_header,
            "MT{} should expose a format table header",
            candidate.message
        );
        assert!(
            fields > 0,
            "MT{} should expose at least one format row",
            candidate.message
        );
        assert!(
            tags > 0,
            "MT{} should expose at least one tag row",
            candidate.message
        );
    }
}
