use swift_core::{parse_message, ParseDiagnostic};

fn diagnostic_offset(diagnostic: &ParseDiagnostic<'_>) -> Option<usize> {
    match diagnostic {
        ParseDiagnostic::ExpectedBlockOpen { offset }
        | ParseDiagnostic::MissingBlockSeparator { offset }
        | ParseDiagnostic::UnclosedBlock { offset }
        | ParseDiagnostic::UnclosedTextBlock { offset }
        | ParseDiagnostic::MalformedFieldTag { offset } => Some(*offset),
        ParseDiagnostic::LimitExceeded { .. }
        | ParseDiagnostic::MismatchedSequenceEnd { .. }
        | ParseDiagnostic::UnclosedSequence { .. } => None,
    }
}

#[test]
fn block_diagnostic_offsets_are_message_relative() {
    let cases: &[(&[u8], usize)] = &[
        (b"prefix{1ABC}", 6),
        (b"prefix{1:ABC", 6),
        (b"prefix{4:\n:20C::SEME//ABC123\n", 6),
    ];

    for (message, expected_offset) in cases {
        let parsed = parse_message(message);
        let actual = parsed
            .diagnostics
            .iter()
            .find_map(diagnostic_offset)
            .expect("case should emit an offset diagnostic");

        assert_eq!(actual, *expected_offset);
    }
}

#[test]
fn malformed_field_tag_offset_is_message_relative() {
    let message = b"prefix{1:F01BANKBEBBAXXX0000000000}{4:\n:20C::SEME//ABC123\n:BROKEN\n-}";
    let expected_offset = message
        .windows(b":BROKEN".len())
        .position(|window| window == b":BROKEN")
        .expect("fixture contains malformed field start");

    let parsed = parse_message(message);

    assert_eq!(
        parsed.diagnostics,
        vec![ParseDiagnostic::MalformedFieldTag {
            offset: expected_offset,
        }]
    );
}
