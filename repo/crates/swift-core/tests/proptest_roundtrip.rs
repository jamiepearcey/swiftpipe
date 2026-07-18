use proptest::prelude::*;
use swift_core::{parse_message, BlockId};

#[derive(Debug, Clone)]
enum FieldTree {
    Field {
        tag: String,
        value: String,
    },
    Sequence {
        name: String,
        fields: Vec<FieldTree>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExpectedField {
    tag: String,
    qualifier: Option<String>,
    value: String,
    sequence_path: Vec<(String, u32)>,
}

type ExpectedPath = Vec<(String, u32)>;
type OccurrenceCounts = Vec<(ExpectedPath, String, u32)>;

fn atom() -> impl Strategy<Value = String> {
    "[A-Z0-9]{1,12}"
}

fn qualifier_value() -> impl Strategy<Value = String> {
    ("[A-Z0-9]{4}", "[A-Z0-9]{1,16}")
        .prop_map(|(qualifier, value)| format!(":{qualifier}//{value}"))
}

fn plain_field() -> impl Strategy<Value = FieldTree> {
    prop_oneof![
        (Just("20C".to_string()), qualifier_value()),
        (Just("95P".to_string()), qualifier_value()),
        (Just("23G".to_string()), atom()),
        (Just("35B".to_string()), atom()),
        (Just("98A".to_string()), atom()),
    ]
    .prop_map(|(tag, value)| FieldTree::Field { tag, value })
}

fn field_tree() -> impl Strategy<Value = Vec<FieldTree>> {
    let leaf = plain_field();
    leaf.prop_recursive(4, 48, 4, |inner| {
        (atom(), prop::collection::vec(inner, 0..6))
            .prop_map(|(name, fields)| FieldTree::Sequence { name, fields })
    })
    .prop_flat_map(|tree| prop::collection::vec(Just(tree), 1..10))
}

fn render_fields(fields: &[FieldTree], output: &mut String) {
    for field in fields {
        match field {
            FieldTree::Field { tag, value } => {
                output.push(':');
                output.push_str(tag);
                output.push(':');
                output.push_str(value);
                output.push('\n');
            }
            FieldTree::Sequence { name, fields } => {
                output.push_str(":16R:");
                output.push_str(name);
                output.push('\n');
                render_fields(fields, output);
                output.push_str(":16S:");
                output.push_str(name);
                output.push('\n');
            }
        }
    }
}

fn rendered_message(fields: &[FieldTree]) -> Vec<u8> {
    let mut message =
        String::from("{1:F01BANKBEBBAXXX0000000000}{2:I540BANKDEFFXXXXN}{3:{108:PROPTEST}}{4:\n");
    render_fields(fields, &mut message);
    message.push_str("-}{5:{CHK:123456789ABC}}");
    message.into_bytes()
}

fn expected_fields(fields: &[FieldTree]) -> Vec<ExpectedField> {
    let mut expected = Vec::new();
    let mut stack = Vec::new();
    let mut counts = Vec::new();
    flatten_expected(fields, &mut stack, &mut counts, &mut expected);
    expected
}

fn flatten_expected(
    fields: &[FieldTree],
    stack: &mut ExpectedPath,
    counts: &mut OccurrenceCounts,
    expected: &mut Vec<ExpectedField>,
) {
    for field in fields {
        match field {
            FieldTree::Field { tag, value } => expected.push(ExpectedField {
                tag: tag.clone(),
                qualifier: expected_qualifier(value),
                value: value.clone(),
                sequence_path: stack.clone(),
            }),
            FieldTree::Sequence { name, fields } => {
                expected.push(ExpectedField {
                    tag: "16R".to_string(),
                    qualifier: None,
                    value: name.clone(),
                    sequence_path: stack.clone(),
                });
                let occurrence = next_occurrence(stack, name, counts);
                stack.push((name.clone(), occurrence));
                flatten_expected(fields, stack, counts, expected);
                expected.push(ExpectedField {
                    tag: "16S".to_string(),
                    qualifier: None,
                    value: name.clone(),
                    sequence_path: stack.clone(),
                });
                stack.pop();
            }
        }
    }
}

fn expected_qualifier(value: &str) -> Option<String> {
    let value = value.as_bytes();
    if value.len() >= 7 && value[0] == b':' && value[5..7] == *b"//" {
        Some(String::from_utf8(value[1..5].to_vec()).expect("qualifier is ascii"))
    } else {
        None
    }
}

fn next_occurrence(stack: &[(String, u32)], name: &str, counts: &mut OccurrenceCounts) -> u32 {
    if let Some((_, _, count)) = counts
        .iter_mut()
        .find(|(existing_stack, existing_name, _)| existing_stack == stack && existing_name == name)
    {
        let occurrence = *count;
        *count += 1;
        occurrence
    } else {
        counts.push((stack.to_vec(), name.to_string(), 1));
        0
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn random_valid_messages_parse_without_diagnostics(fields in field_tree()) {
        let message = rendered_message(&fields);
        let parsed = parse_message(&message);

        prop_assert_eq!(&parsed.diagnostics, &[]);
        prop_assert_eq!(parsed.blocks.len(), 5);
        prop_assert_eq!(parsed.blocks[0].id, BlockId::BasicHeader);
        prop_assert_eq!(parsed.blocks[1].id, BlockId::ApplicationHeader);
        prop_assert_eq!(parsed.blocks[2].id, BlockId::UserHeader);
        prop_assert_eq!(parsed.blocks[3].id, BlockId::Text);
        prop_assert_eq!(parsed.blocks[4].id, BlockId::Trailer);

        let actual: Vec<_> = parsed.fields.iter().map(|field| ExpectedField {
            tag: String::from_utf8(field.tag.to_vec()).expect("generated tags are ascii"),
            qualifier: field
                .qualifier
                .map(|qualifier| String::from_utf8(qualifier.to_vec()).expect("generated qualifiers are ascii")),
            value: String::from_utf8(field.value.to_vec()).expect("generated values are ascii"),
            sequence_path: field
                .sequence_path
                .iter()
                .map(|frame| {
                    (
                        String::from_utf8(frame.name.to_vec()).expect("generated sequence names are ascii"),
                        frame.occurrence,
                    )
                })
                .collect(),
        }).collect();

        prop_assert_eq!(actual, expected_fields(&fields));
    }
}
