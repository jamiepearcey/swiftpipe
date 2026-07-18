use std::fs;
use std::path::{Path, PathBuf};

fn examples_dir() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .to_path_buf()
}

fn uhb_spec_files() -> Vec<PathBuf> {
    let dir = examples_dir().join("examples/.uhb");
    let mut files = Vec::new();

    let entries = fs::read_dir(dir).expect("uhb cache directory should be readable");
    for entry in entries {
        let entry = entry.expect("directory entry should be readable");
        let path = entry.path();
        if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
            if name.starts_with("finmt") && name.ends_with(".md") {
                files.push(path);
            }
        }
    }

    files.sort();
    files
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

#[test]
fn uhb_message_specs_are_parseable_with_table_parser() {
    let mut parsed = 0;

    for path in uhb_spec_files() {
        let stem = path
            .file_stem()
            .and_then(|v| v.to_str())
            .unwrap_or_default();
        let mt = stem.trim_start_matches("finmt");

        let (fields, tags, has_known_header) = parse_uhb_file_with_fallback(&path);
        assert!(
            has_known_header,
            "MT{} should expose a format table header",
            mt
        );
        assert!(fields > 0, "MT{} should expose at least one format row", mt);
        assert!(tags > 0, "MT{} should expose at least one tag row", mt);
        parsed += 1;
    }

    assert!(parsed > 0, "expected at least one UHB spec in cache");
}
