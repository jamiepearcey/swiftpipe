//! Upstream and synthetic FIN fixtures (see `examples/public-swift-samples/{upstream,synthetic}`)
//! must parse cleanly and satisfy the bundled schema for their message type.

use std::fs;
use std::path::PathBuf;

use swift_core::{
    message_type_of, parse_message_with_sequences, split_bare_statements, split_messages,
};
use swift_schema::{anchored_sequences, match_and_parse_message, SchemaCatalog};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn load_catalog() -> SchemaCatalog {
    let mut field_types: Vec<swift_schema::FieldTypeSchema> = Vec::new();
    let mut messages = Vec::new();
    let mut paths = fs::read_dir(root().join("examples/schemas"))
        .expect("schemas directory should be readable")
        .map(|entry| entry.expect("schema entry").path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("yaml"))
        .collect::<Vec<_>>();
    paths.sort();
    for path in paths {
        let mut schema =
            SchemaCatalog::from_yaml_str(&fs::read_to_string(path).expect("schema readable"))
                .expect("schema parses");
        for field_type in schema.field_types.drain(..) {
            if !field_types
                .iter()
                .any(|existing| existing.name == field_type.name)
            {
                field_types.push(field_type);
            }
        }
        messages.append(&mut schema.messages);
    }
    SchemaCatalog {
        field_types,
        messages,
    }
}

#[test]
fn upstream_fixtures_match_their_schemas() {
    let catalog = load_catalog();
    let mut files = ["upstream", "synthetic", "generated"]
        .iter()
        .flat_map(|sub| {
            fs::read_dir(root().join("examples/public-swift-samples").join(sub))
                .expect("fixture directory is readable")
        })
        .map(|entry| entry.expect("fixture entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext == "fin" || ext == "rje" || ext == "sta")
        })
        .collect::<Vec<_>>();
    files.sort();
    assert!(!files.is_empty(), "no upstream fixtures found");

    for file in files {
        let name = file.file_name().unwrap().to_string_lossy().into_owned();
        let raw = fs::read(&file).expect("fixture readable");
        let mut messages: Vec<Vec<u8>> = split_messages(&raw)
            .into_iter()
            .map(<[u8]>::to_vec)
            .collect();
        if messages.is_empty() {
            messages = split_bare_statements(&raw)
                .into_iter()
                .map(|(_, fin)| fin.into_bytes())
                .collect();
        }
        assert!(!messages.is_empty(), "{name}: no messages found");
        for (n, body) in messages.iter().map(Vec::as_slice).enumerate() {
            let name = format!("{name}#{n}");
            let message_type = message_type_of(body).expect("message type in block 2");
            let schema = catalog
                .messages
                .iter()
                .find(|message| message.message == format!("MT{message_type}"))
                .unwrap_or_else(|| panic!("{name}: no schema for MT{message_type}"));

            let anchored = anchored_sequences(schema);
            let parsed = parse_message_with_sequences(body, &anchored);
            // Upstream negative case: the trailer block is truncated to a lone `{`.
            let known_malformed = name == "prowide-core_MT103-out-ack.rje#10";
            if known_malformed {
                assert_eq!(
                    parsed.diagnostics.len(),
                    1,
                    "{name}: {:?}",
                    parsed.diagnostics
                );
            }
            assert!(
                known_malformed || parsed.diagnostics.is_empty(),
                "{name}: {:?}",
                parsed.diagnostics
            );
            let matched = match_and_parse_message(&catalog, schema, &parsed);
            assert!(
                matched.parse_errors.is_empty(),
                "{name}: field parse errors"
            );
            assert!(
                matched.missing_required.is_empty(),
                "{name}: missing {:?}",
                matched.missing_required
            );
            assert!(
                matched.cardinality_violations.is_empty(),
                "{name}: cardinality {:?}",
                matched.cardinality_violations
            );
            assert!(
                matched.sequence_issues.is_empty(),
                "{name}: sequences {:?}",
                matched.sequence_issues
            );
        }
    }
}
