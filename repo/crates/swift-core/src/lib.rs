#![forbid(unsafe_code)]
#![deny(warnings, rust_2018_idioms, missing_debug_implementations)]
#![warn(clippy::pedantic)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::similar_names
)]

//! Zero-copy structural parsing for SWIFT FIN messages.
//!
//! This crate deliberately stops at message structure: envelope blocks, block 4
//! fields, qualifiers, and 16R/16S sequence paths. Schema matching and database
//! output are layered on top by later crates.

use memchr::memchr;
use smallvec::SmallVec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockId<'a> {
    BasicHeader,
    ApplicationHeader,
    UserHeader,
    Text,
    Trailer,
    Other(&'a [u8]),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwiftBlock<'a> {
    pub id: BlockId<'a>,
    pub raw: &'a [u8],
    pub content: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SequenceFrame<'a> {
    pub name: &'a [u8],
    pub occurrence: u32,
}

pub type SequencePath<'a> = SmallVec<[SequenceFrame<'a>; 8]>;

/// A schema-declared **anchored** sequence: a repeating field group that has
/// NO `:16R:`/`:16S:` wrapper in the wire format (e.g. MT940/942/950's
/// Statement Line group — tag `61` starts a new occurrence, an optional `86`
/// belongs to it, and any other tag closes the group). Root-scope only: an
/// anchored sequence is only recognised while no `:16R:` sequence is open.
/// Generalizes the proven grouping logic in `swift_mt940::parse_mt940` so the
/// schema-driven pipeline can express message types that don't use 16R/16S.
#[derive(Debug, Clone)]
pub struct AnchoredSequence<'a> {
    pub name: &'a [u8],
    pub anchor_tag: &'a [u8],
    pub member_tags: Vec<&'a [u8]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldSlice<'a> {
    pub tag: &'a [u8],
    pub qualifier: Option<&'a [u8]>,
    pub value: &'a [u8],
    pub sequence_path: SequencePath<'a>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseLimits {
    pub max_total_bytes: usize,
    pub max_block_bytes: usize,
    pub max_fields: usize,
    pub max_sequence_depth: u32,
}

impl ParseLimits {
    pub const fn default_strict() -> Self {
        Self {
            max_total_bytes: 10 * 1024 * 1024,
            max_block_bytes: 4 * 1024 * 1024,
            max_fields: 10_000,
            max_sequence_depth: 16,
        }
    }

    pub const fn lenient() -> Self {
        Self {
            max_total_bytes: usize::MAX,
            max_block_bytes: usize::MAX,
            max_fields: usize::MAX,
            max_sequence_depth: u32::MAX,
        }
    }
}

impl Default for ParseLimits {
    fn default() -> Self {
        Self::default_strict()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseLimitKind {
    TotalBytes,
    BlockBytes,
    Fields,
    SequenceDepth,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseDiagnostic<'a> {
    LimitExceeded {
        kind: ParseLimitKind,
        limit: usize,
        observed: usize,
    },
    /// The parser expected a `{` at `offset`; offsets are byte indexes in the
    /// message passed to `parse_message`.
    ExpectedBlockOpen {
        offset: usize,
    },
    /// A block opened at `offset` without a `:` separator; offsets are byte
    /// indexes in the message passed to `parse_message`.
    MissingBlockSeparator {
        offset: usize,
    },
    /// A non-text block opened at `offset` without a closing `}`; offsets are
    /// byte indexes in the message passed to `parse_message`.
    UnclosedBlock {
        offset: usize,
    },
    /// Text block 4 opened at `offset` without a closing `-}`; offsets are byte
    /// indexes in the message passed to `parse_message`.
    UnclosedTextBlock {
        offset: usize,
    },
    /// A field tag line started at `offset` but was malformed; offsets are byte
    /// indexes in the message passed to `parse_message`.
    MalformedFieldTag {
        offset: usize,
    },
    MismatchedSequenceEnd {
        expected: Option<&'a [u8]>,
        found: &'a [u8],
    },
    UnclosedSequence {
        name: &'a [u8],
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedMessage<'a> {
    pub blocks: Vec<SwiftBlock<'a>>,
    pub fields: Vec<FieldSlice<'a>>,
    pub diagnostics: Vec<ParseDiagnostic<'a>>,
}

impl<'a> ParsedMessage<'a> {
    pub fn text_block(&self) -> Option<SwiftBlock<'a>> {
        self.blocks
            .iter()
            .copied()
            .find(|block| block.id == BlockId::Text)
    }
}

pub fn parse_message(input: &[u8]) -> ParsedMessage<'_> {
    parse_message_with_limits(input, &ParseLimits::default_strict())
}

pub fn parse_message_with_limits<'a>(input: &'a [u8], limits: &ParseLimits) -> ParsedMessage<'a> {
    parse_message_with_limits_and_sequences(input, limits, &[])
}

/// Like [`parse_message`], but additionally recognises schema-declared
/// [`AnchoredSequence`]s (see its docs) for message types whose repeating
/// groups have no `:16R:`/`:16S:` wrapper. `anchored` is normally empty (every
/// existing schema-driven message keeps its current, unaffected behavior);
/// only message types that declare an anchored sequence use it.
pub fn parse_message_with_sequences<'a>(
    input: &'a [u8],
    anchored: &[AnchoredSequence<'a>],
) -> ParsedMessage<'a> {
    parse_message_with_limits_and_sequences(input, &ParseLimits::default_strict(), anchored)
}

pub fn parse_message_with_limits_and_sequences<'a>(
    input: &'a [u8],
    limits: &ParseLimits,
    anchored: &[AnchoredSequence<'a>],
) -> ParsedMessage<'a> {
    let mut diagnostics = Vec::new();
    if input.len() > limits.max_total_bytes {
        diagnostics.push(ParseDiagnostic::LimitExceeded {
            kind: ParseLimitKind::TotalBytes,
            limit: limits.max_total_bytes,
            observed: input.len(),
        });
        return ParsedMessage {
            blocks: Vec::new(),
            fields: Vec::new(),
            diagnostics,
        };
    }

    let blocks = parse_blocks_with_limits(input, &mut diagnostics, limits);
    let mut fields = Vec::new();

    if let Some(text) = blocks.iter().find(|block| block.id == BlockId::Text) {
        fields = parse_text_fields_with_limits(
            text.content,
            &mut diagnostics,
            limits,
            slice_offset(input, text.content),
            anchored,
        );
    }

    ParsedMessage {
        blocks,
        fields,
        diagnostics,
    }
}

pub fn parse_blocks<'a>(
    input: &'a [u8],
    diagnostics: &mut Vec<ParseDiagnostic<'a>>,
) -> Vec<SwiftBlock<'a>> {
    parse_blocks_with_limits(input, diagnostics, &ParseLimits::lenient())
}

fn parse_blocks_with_limits<'a>(
    input: &'a [u8],
    diagnostics: &mut Vec<ParseDiagnostic<'a>>,
    limits: &ParseLimits,
) -> Vec<SwiftBlock<'a>> {
    let mut blocks = Vec::with_capacity(5);
    let mut offset = 0;

    while offset < input.len() {
        let Some(open_rel) = memchr(b'{', &input[offset..]) else {
            break;
        };
        let open = offset + open_rel;
        if open + 3 > input.len() {
            diagnostics.push(ParseDiagnostic::UnclosedBlock { offset: open });
            break;
        }

        let header = &input[open + 1..];
        let Some(colon_rel) = memchr(b':', header) else {
            diagnostics.push(ParseDiagnostic::MissingBlockSeparator { offset: open });
            break;
        };
        if memchr(b'}', header).is_some_and(|close_rel| close_rel < colon_rel) {
            diagnostics.push(ParseDiagnostic::MissingBlockSeparator { offset: open });
            break;
        }
        let colon = open + 1 + colon_rel;
        let id = &input[open + 1..colon];
        let content_start = colon + 1;

        let Some(close) = find_block_close(input, open, content_start, id, diagnostics) else {
            break;
        };

        let observed_block_bytes = close - open + 1;
        if observed_block_bytes > limits.max_block_bytes {
            diagnostics.push(ParseDiagnostic::LimitExceeded {
                kind: ParseLimitKind::BlockBytes,
                limit: limits.max_block_bytes,
                observed: observed_block_bytes,
            });
            break;
        }

        blocks.push(SwiftBlock {
            id: block_id(id),
            raw: &input[open..=close],
            content: content_slice(input, content_start, close, id),
        });
        offset = close + 1;
    }

    blocks
}

pub fn parse_text_fields<'a>(
    text: &'a [u8],
    diagnostics: &mut Vec<ParseDiagnostic<'a>>,
) -> Vec<FieldSlice<'a>> {
    parse_text_fields_with_limits(text, diagnostics, &ParseLimits::lenient(), 0, &[])
}

fn parse_text_fields_with_limits<'a>(
    text: &'a [u8],
    diagnostics: &mut Vec<ParseDiagnostic<'a>>,
    limits: &ParseLimits,
    base_offset: usize,
    anchored: &[AnchoredSequence<'a>],
) -> Vec<FieldSlice<'a>> {
    let mut fields = Vec::with_capacity(count_field_starts(text, limits.max_fields));
    let mut sequence_stack: SequencePath<'a> = SmallVec::new();
    let mut sequence_counts: SmallVec<[(SequencePath<'a>, &'a [u8], u32); 16]> = SmallVec::new();
    let mut open_anchor: Option<(usize, u32)> = None;
    let mut pending_start = None;
    let mut accepted_fields = 0;

    let mut line_start = 0;
    while line_start < text.len() {
        let line_end =
            memchr(b'\n', &text[line_start..]).map_or(text.len(), |rel| line_start + rel);

        if text.get(line_start) == Some(&b':') {
            if let Some(start) =
                parse_field_start(text, line_start, line_end, diagnostics, base_offset)
            {
                if accepted_fields >= limits.max_fields {
                    diagnostics.push(ParseDiagnostic::LimitExceeded {
                        kind: ParseLimitKind::Fields,
                        limit: limits.max_fields,
                        observed: accepted_fields + 1,
                    });
                    break;
                }

                if let Some(previous) = pending_start.replace(start) {
                    let tag = &text[previous.tag_start..previous.tag_end];
                    let value = trim_field_value(
                        &text[previous.value_start..previous_line_end(text, start.start)],
                    );
                    push_field(
                        tag,
                        value,
                        &mut fields,
                        &mut sequence_stack,
                        &mut sequence_counts,
                        anchored,
                        &mut open_anchor,
                        diagnostics,
                        limits,
                    );
                }
                accepted_fields += 1;
            }
        }

        if line_end == text.len() {
            break;
        }
        line_start = line_end + 1;
    }

    if let Some(start) = pending_start {
        let tag = &text[start.tag_start..start.tag_end];
        let value = trim_field_value(&text[start.value_start..text.len()]);
        push_field(
            tag,
            value,
            &mut fields,
            &mut sequence_stack,
            &mut sequence_counts,
            anchored,
            &mut open_anchor,
            diagnostics,
            limits,
        );
    }

    for frame in sequence_stack.iter().rev() {
        diagnostics.push(ParseDiagnostic::UnclosedSequence { name: frame.name });
    }

    fields
}

#[allow(clippy::too_many_arguments)]
fn push_field<'a>(
    tag: &'a [u8],
    value: &'a [u8],
    fields: &mut Vec<FieldSlice<'a>>,
    sequence_stack: &mut SequencePath<'a>,
    sequence_counts: &mut SmallVec<[(SequencePath<'a>, &'a [u8], u32); 16]>,
    anchored: &[AnchoredSequence<'a>],
    open_anchor: &mut Option<(usize, u32)>,
    diagnostics: &mut Vec<ParseDiagnostic<'a>>,
    limits: &ParseLimits,
) {
    if tag == b"16S" {
        let found = trim_ascii(value);
        match sequence_stack.last().copied() {
            Some(frame) if frame.name == found => {
                fields.push(FieldSlice {
                    tag,
                    qualifier: extract_qualifier(value),
                    value,
                    sequence_path: sequence_stack.clone(),
                });
                sequence_stack.pop();
            }
            Some(frame) => diagnostics.push(ParseDiagnostic::MismatchedSequenceEnd {
                expected: Some(frame.name),
                found,
            }),
            None => diagnostics.push(ParseDiagnostic::MismatchedSequenceEnd {
                expected: None,
                found,
            }),
        }
        return;
    }

    // Anchored ("implicit") sequences — root scope only, no 16R/16S wrapper.
    // The anchor tag starts (or restarts) an occurrence; declared member tags
    // stay inside it; any other tag closes it. See `AnchoredSequence`.
    let anchor_frame: Option<SequenceFrame<'a>> =
        if sequence_stack.is_empty() && !anchored.is_empty() {
            if let Some(idx) = anchored.iter().position(|a| a.anchor_tag == tag) {
                let occurrence =
                    next_sequence_occurrence(sequence_stack, anchored[idx].name, sequence_counts);
                *open_anchor = Some((idx, occurrence));
                Some(SequenceFrame {
                    name: anchored[idx].name,
                    occurrence,
                })
            } else if let Some((idx, occurrence)) = *open_anchor {
                if anchored[idx].member_tags.contains(&tag) {
                    Some(SequenceFrame {
                        name: anchored[idx].name,
                        occurrence,
                    })
                } else {
                    *open_anchor = None;
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

    let sequence_path = match anchor_frame {
        Some(frame) => {
            let mut path = sequence_stack.clone();
            path.push(frame);
            path
        }
        None => sequence_stack.clone(),
    };

    fields.push(FieldSlice {
        tag,
        qualifier: extract_qualifier(value),
        value,
        sequence_path,
    });

    if tag == b"16R" {
        let name = trim_ascii(value);
        if sequence_stack.len() >= limits.max_sequence_depth as usize {
            diagnostics.push(ParseDiagnostic::LimitExceeded {
                kind: ParseLimitKind::SequenceDepth,
                limit: limits.max_sequence_depth as usize,
                observed: sequence_stack.len() + 1,
            });
            return;
        }
        let occurrence = next_sequence_occurrence(sequence_stack, name, sequence_counts);
        sequence_stack.push(SequenceFrame { name, occurrence });
    }
}

fn block_id(id: &[u8]) -> BlockId<'_> {
    match id {
        b"1" => BlockId::BasicHeader,
        b"2" => BlockId::ApplicationHeader,
        b"3" => BlockId::UserHeader,
        b"4" => BlockId::Text,
        b"5" => BlockId::Trailer,
        other => BlockId::Other(other),
    }
}

fn find_block_close<'a>(
    input: &'a [u8],
    open: usize,
    content_start: usize,
    id: &[u8],
    diagnostics: &mut Vec<ParseDiagnostic<'a>>,
) -> Option<usize> {
    if id == b"4" {
        let mut cursor = content_start;
        while cursor + 1 < input.len() {
            if input[cursor] == b'-' && input[cursor + 1] == b'}' {
                return Some(cursor + 1);
            }
            cursor += 1;
        }
        diagnostics.push(ParseDiagnostic::UnclosedTextBlock { offset: open });
        return None;
    }

    let mut depth = 0_u32;
    for (idx, byte) in input[open..].iter().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + idx);
                }
            }
            _ => {}
        }
    }

    diagnostics.push(ParseDiagnostic::UnclosedBlock { offset: open });
    None
}

fn content_slice<'a>(input: &'a [u8], content_start: usize, close: usize, id: &[u8]) -> &'a [u8] {
    if id == b"4" && close > content_start && input[close - 1] == b'-' {
        &input[content_start..close - 1]
    } else {
        &input[content_start..close]
    }
}

fn slice_offset(input: &[u8], slice: &[u8]) -> usize {
    slice.as_ptr() as usize - input.as_ptr() as usize
}

#[derive(Debug, Clone, Copy)]
struct FieldStart {
    start: usize,
    tag_start: usize,
    tag_end: usize,
    value_start: usize,
}

fn count_field_starts(text: &[u8], max_fields: usize) -> usize {
    let mut count = 0;
    let mut line_start = 0;

    while line_start < text.len() {
        let line_end =
            memchr(b'\n', &text[line_start..]).map_or(text.len(), |rel| line_start + rel);

        if text.get(line_start) == Some(&b':') && is_valid_field_start(text, line_start, line_end) {
            if count >= max_fields {
                break;
            }
            count += 1;
        }

        if line_end == text.len() {
            break;
        }
        line_start = line_end + 1;
    }

    count
}

fn is_valid_field_start(text: &[u8], line_start: usize, line_end: usize) -> bool {
    let tag_start = line_start + 1;
    let Some(second_colon_rel) = memchr(b':', &text[tag_start..line_end]) else {
        return false;
    };
    let tag_end = tag_start + second_colon_rel;
    let tag = &text[tag_start..tag_end];

    (2..=4).contains(&tag.len()) && tag.iter().all(u8::is_ascii_alphanumeric)
}

fn parse_field_start<'a>(
    text: &'a [u8],
    line_start: usize,
    line_end: usize,
    diagnostics: &mut Vec<ParseDiagnostic<'a>>,
    base_offset: usize,
) -> Option<FieldStart> {
    let tag_start = line_start + 1;
    let Some(second_colon_rel) = memchr(b':', &text[tag_start..line_end]) else {
        diagnostics.push(ParseDiagnostic::MalformedFieldTag {
            offset: base_offset + line_start,
        });
        return None;
    };
    let tag_end = tag_start + second_colon_rel;
    let tag = &text[tag_start..tag_end];

    if !(2..=4).contains(&tag.len()) || !tag.iter().all(u8::is_ascii_alphanumeric) {
        diagnostics.push(ParseDiagnostic::MalformedFieldTag {
            offset: base_offset + line_start,
        });
        return None;
    }

    Some(FieldStart {
        start: line_start,
        tag_start,
        tag_end,
        value_start: tag_end + 1,
    })
}

fn previous_line_end(text: &[u8], next_field_start: usize) -> usize {
    if next_field_start >= 1 && text[next_field_start - 1] == b'\n' {
        next_field_start - 1
    } else {
        next_field_start
    }
}

fn trim_field_value(value: &[u8]) -> &[u8] {
    let value = trim_ascii(value);
    if let Some(stripped) = value.strip_suffix(b"\r") {
        stripped
    } else {
        value
    }
}

fn trim_ascii(mut value: &[u8]) -> &[u8] {
    while let Some(first) = value.first() {
        if first.is_ascii_whitespace() {
            value = &value[1..];
        } else {
            break;
        }
    }

    while let Some(last) = value.last() {
        if last.is_ascii_whitespace() {
            value = &value[..value.len() - 1];
        } else {
            break;
        }
    }

    value
}

fn extract_qualifier(value: &[u8]) -> Option<&[u8]> {
    let rest = value.strip_prefix(b":")?;
    if rest.len() < 6 {
        return None;
    }
    let qualifier = &rest[..4];
    if !qualifier.iter().all(u8::is_ascii_alphanumeric) {
        return None;
    }
    if rest.get(4..6) == Some(b"//") {
        Some(qualifier)
    } else {
        None
    }
}

fn next_sequence_occurrence<'a>(
    stack: &SequencePath<'a>,
    name: &'a [u8],
    counts: &mut SmallVec<[(SequencePath<'a>, &'a [u8], u32); 16]>,
) -> u32 {
    if let Some((_, _, count)) = counts
        .iter_mut()
        .find(|(existing_stack, existing_name, _)| {
            existing_stack == stack && *existing_name == name
        })
    {
        let occurrence = *count;
        *count += 1;
        occurrence
    } else {
        counts.push((stack.clone(), name, 1));
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_semt() -> &'static [u8] {
        br"{1:F01BANKBEBBAXXX0000000000}{2:I540BANKDEFFXXXXN}{3:{108:ABC123}{121:550e8400-e29b-41d4-a716-446655440000}}{4:
:16R:GENL
:20C::SEME//ABC123
:23G:NEWM
:16R:LINK
:20C::RELA//REL1
:16S:LINK
:16R:LINK
:20C::RELA//REL2
:16S:LINK
:16S:GENL
-}{5:{CHK:123456789ABC}}"
    }

    #[test]
    fn parses_envelope_blocks() {
        let parsed = parse_message(sample_semt());

        assert!(parsed.diagnostics.is_empty());
        assert_eq!(parsed.blocks.len(), 5);
        assert_eq!(parsed.blocks[0].id, BlockId::BasicHeader);
        assert_eq!(parsed.blocks[3].id, BlockId::Text);
        assert_eq!(
            parsed
                .text_block()
                .map(|block| block.content.starts_with(b"\n:16R")),
            Some(true)
        );
    }

    #[test]
    fn reports_total_byte_limit_exceeded() {
        let limits = ParseLimits {
            max_total_bytes: 8,
            ..ParseLimits::default_strict()
        };

        let parsed = parse_message_with_limits(sample_semt(), &limits);

        assert_eq!(
            parsed.diagnostics,
            vec![ParseDiagnostic::LimitExceeded {
                kind: ParseLimitKind::TotalBytes,
                limit: 8,
                observed: sample_semt().len(),
            }]
        );
        assert!(parsed.blocks.is_empty());
        assert!(parsed.fields.is_empty());
    }

    #[test]
    fn reports_block_byte_limit_exceeded() {
        let limits = ParseLimits {
            max_block_bytes: 8,
            ..ParseLimits::lenient()
        };

        let parsed = parse_message_with_limits(sample_semt(), &limits);

        assert_eq!(
            parsed.diagnostics,
            vec![ParseDiagnostic::LimitExceeded {
                kind: ParseLimitKind::BlockBytes,
                limit: 8,
                observed: 29,
            }]
        );
        assert!(parsed.blocks.is_empty());
    }

    #[test]
    fn reports_field_count_limit_exceeded() {
        let limits = ParseLimits {
            max_fields: 1,
            ..ParseLimits::lenient()
        };
        let message = br"{4:
:20C::SEME//ABC123
:23G:NEWM
-}";

        let parsed = parse_message_with_limits(message, &limits);

        assert_eq!(parsed.fields.len(), 1);
        assert_eq!(
            parsed.diagnostics,
            vec![ParseDiagnostic::LimitExceeded {
                kind: ParseLimitKind::Fields,
                limit: 1,
                observed: 2,
            }]
        );
    }

    #[test]
    fn reports_sequence_depth_limit_exceeded() {
        let limits = ParseLimits {
            max_sequence_depth: 1,
            ..ParseLimits::lenient()
        };
        let message = br"{4:
:16R:GENL
:16R:LINK
:16S:LINK
:16S:GENL
-}";

        let parsed = parse_message_with_limits(message, &limits);

        assert_eq!(
            parsed.diagnostics,
            vec![
                ParseDiagnostic::LimitExceeded {
                    kind: ParseLimitKind::SequenceDepth,
                    limit: 1,
                    observed: 2,
                },
                ParseDiagnostic::MismatchedSequenceEnd {
                    expected: Some(&b"GENL"[..]),
                    found: &b"LINK"[..],
                },
            ]
        );
    }

    #[test]
    fn malformed_block_header_does_not_capture_later_separator() {
        let message = br"{1F01BANKBEBBAXXX0000000000}{2:I321BANKDEFFXXXXN}{4:
:20C::SEME//BROKEN
-}
";

        let parsed = parse_message(message);

        assert_eq!(
            parsed.diagnostics,
            vec![ParseDiagnostic::MissingBlockSeparator { offset: 0 }]
        );
        assert!(parsed.blocks.is_empty());
        assert!(parsed.fields.is_empty());
    }

    #[test]
    fn parses_fields_and_qualifiers() {
        let parsed = parse_message(sample_semt());

        let seme = parsed
            .fields
            .iter()
            .find(|field| field.tag == b"20C" && field.qualifier == Some(&b"SEME"[..]))
            .expect("SEME reference field");

        assert_eq!(seme.value, b":SEME//ABC123");
        assert_eq!(seme.sequence_path.len(), 1);
        assert_eq!(seme.sequence_path[0].name, b"GENL");
    }

    #[test]
    fn tracks_repeated_nested_sequences() {
        let parsed = parse_message(sample_semt());
        let rela_fields: Vec<_> = parsed
            .fields
            .iter()
            .filter(|field| field.tag == b"20C" && field.qualifier == Some(&b"RELA"[..]))
            .collect();

        assert_eq!(rela_fields.len(), 2);
        assert_eq!(rela_fields[0].sequence_path.len(), 2);
        assert_eq!(rela_fields[0].sequence_path[0].name, b"GENL");
        assert_eq!(rela_fields[0].sequence_path[1].name, b"LINK");
        assert_eq!(rela_fields[1].sequence_path[1].occurrence, 1);
    }

    #[test]
    fn reports_mismatched_sequence_end() {
        let message = br"{1:F01BANKBEBBAXXX0000000000}{2:I540BANKDEFFXXXXN}{4:
:16R:GENL
:20C::SEME//ABC123
:16S:LINK
-}";

        let parsed = parse_message(message);

        assert_eq!(
            parsed.diagnostics,
            vec![
                ParseDiagnostic::MismatchedSequenceEnd {
                    expected: Some(&b"GENL"[..]),
                    found: &b"LINK"[..],
                },
                ParseDiagnostic::UnclosedSequence { name: &b"GENL"[..] },
            ]
        );
    }

    #[test]
    fn keeps_continuation_lines_in_field_value() {
        let message = br"{4:
:70E::SPRO//FIRST LINE
SECOND LINE
:16R:GENL
:16S:GENL
-}";

        let parsed = parse_message(message);
        let narrative = parsed
            .fields
            .iter()
            .find(|field| field.tag == b"70E")
            .expect("narrative field");

        assert_eq!(narrative.value, b":SPRO//FIRST LINE\nSECOND LINE");
        assert_eq!(narrative.qualifier, Some(&b"SPRO"[..]));
    }

    #[test]
    fn resets_child_sequence_occurrences_for_each_parent_occurrence() {
        let message = br"{4:
:16R:STAT
:16R:TRAN
:16R:TRANSDET
:20C::RELA//REL1
:16S:TRANSDET
:16S:TRAN
:16R:TRAN
:16R:TRANSDET
:20C::RELA//REL2
:16S:TRANSDET
:16S:TRAN
:16S:STAT
-}";

        let parsed = parse_message(message);
        let rela_fields: Vec<_> = parsed
            .fields
            .iter()
            .filter(|field| field.tag == b"20C" && field.qualifier == Some(&b"RELA"[..]))
            .collect();

        assert_eq!(rela_fields.len(), 2);
        assert_eq!(rela_fields[0].sequence_path[1].occurrence, 0);
        assert_eq!(rela_fields[0].sequence_path[2].occurrence, 0);
        assert_eq!(rela_fields[1].sequence_path[1].occurrence, 1);
        assert_eq!(rela_fields[1].sequence_path[2].occurrence, 0);
    }

    #[test]
    fn parses_public_midclear_samples() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("examples/public-swift-samples");
        let mut samples = std::fs::read_dir(&root)
            .expect("fixture directory is readable")
            .map(|entry| entry.expect("fixture entry is readable").path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "fin"))
            .collect::<Vec<_>>();
        samples.sort();

        for sample in samples {
            let body = std::fs::read(&sample).expect("sample is readable");
            let parsed = parse_message(&body);
            let name = sample.file_name().unwrap().to_string_lossy();
            assert!(
                parsed.diagnostics.is_empty(),
                "{name} diagnostics: {:?}",
                parsed.diagnostics
            );
            assert!(!parsed.fields.is_empty(), "{name} should contain fields");
        }
    }

    // ---- Anchored ("implicit") sequences: MT940's Statement Line group ----
    // MT940 has no :16R:/:16S: at all — the 61/86 grouping is by tag adjacency.
    // These prove `AnchoredSequence` reproduces the exact grouping semantics
    // already proven correct in `swift_mt940::parse_mt940`.

    fn mt940_anchor() -> AnchoredSequence<'static> {
        AnchoredSequence {
            name: b"ENTRY",
            anchor_tag: b"61",
            member_tags: vec![b"86"],
        }
    }

    #[test]
    fn anchored_sequence_groups_each_entry_with_its_own_narrative() {
        let message = b"{4:\n:20:STMT1\n:25:ACC1\n:28C:1/1\n:60F:C260512EUR100000,00\n\
                        :61:2605130513C25000,00NTRFCUST-A//BANKREF-A\n:86:Coupon receipt\n\
                        :61:2605130513D5000,00NCHGCUST-B//BANKREF-B\n:86:Custody fee\n\
                        :62F:C260513EUR120000,00\n:64:C260513EUR120000,00\n-}";
        let parsed = parse_message_with_sequences(message, &[mt940_anchor()]);
        assert!(
            parsed.diagnostics.is_empty(),
            "diagnostics: {:?}",
            parsed.diagnostics
        );

        let by_tag_and_path: Vec<(&str, &[u8], Option<u32>)> = parsed
            .fields
            .iter()
            .map(|f| {
                let tag = std::str::from_utf8(f.tag).unwrap();
                let occurrence = f.sequence_path.last().map(|frame| frame.occurrence);
                (tag, f.value, occurrence)
            })
            .collect();

        // Root-level fields (20/25/28C/60F) carry no anchored scope.
        for (tag, _, occurrence) in &by_tag_and_path[..4] {
            assert_eq!(*occurrence, None, "{tag} should be at root scope");
        }
        // Entry 1: :61: and its :86: share occurrence 0.
        assert_eq!(by_tag_and_path[4].0, "61");
        assert_eq!(by_tag_and_path[4].2, Some(0));
        assert_eq!(by_tag_and_path[5].0, "86");
        assert_eq!(by_tag_and_path[5].2, Some(0));
        // Entry 2: :61: and its :86: share occurrence 1 (not 0).
        assert_eq!(by_tag_and_path[6].0, "61");
        assert_eq!(by_tag_and_path[6].2, Some(1));
        assert_eq!(by_tag_and_path[7].0, "86");
        assert_eq!(by_tag_and_path[7].2, Some(1));
        // Closing fields (62F/64) are back at root — the group closed on :62F:.
        assert_eq!(by_tag_and_path[8].0, "62F");
        assert_eq!(by_tag_and_path[8].2, None);
        assert_eq!(by_tag_and_path[9].0, "64");
        assert_eq!(by_tag_and_path[9].2, None);
    }

    #[test]
    fn anchored_sequence_does_not_attach_trailer_narrative_to_last_entry() {
        // A standalone trailing :86: (after the closing balance) is a SEPARATE,
        // unattached narrative per the real MT940 spec — it must NOT be swept
        // into the last entry's occurrence just because :86: is a member tag.
        let message = b"{4:\n:20:STMT1\n:25:ACC1\n:28C:1/1\n:60F:C260512EUR100000,00\n\
                        :61:2605130513C25000,00NTRFCUST-A//BANKREF-A\n:86:Coupon receipt\n\
                        :62F:C260513EUR120000,00\n:86:Trailer narrative\n-}";
        let parsed = parse_message_with_sequences(message, &[mt940_anchor()]);
        assert!(parsed.diagnostics.is_empty());

        let trailer = parsed.fields.last().expect("trailer field present");
        assert_eq!(trailer.tag, b"86");
        assert!(
            trailer.sequence_path.is_empty(),
            "trailer :86: must be at root scope, not attached to the last entry"
        );
    }

    #[test]
    fn anchored_sequence_closes_when_a_wrapped_sequence_opens() {
        // A schema could plausibly mix an anchored group (MT940-style 61/86)
        // with an ordinary 16R/16S-wrapped sequence elsewhere in the same
        // message. The wrapped sequence opening while the anchor is still
        // "open" must close the anchor — it must NOT leave `open_anchor`
        // stale so that a later root-scope :86: (after the wrapper closes)
        // gets swept into the earlier anchored occurrence.
        let message = b"{4:\n:61:2605130513C25000,00NTRFCUST-A//BANKREF-A\n:86:Coupon receipt\n\
                        :16R:LINK\n:20C::RELA//REL1\n:16S:LINK\n\
                        :86:Trailer narrative\n-}";
        let parsed = parse_message_with_sequences(message, &[mt940_anchor()]);
        assert!(
            parsed.diagnostics.is_empty(),
            "diagnostics: {:?}",
            parsed.diagnostics
        );

        let trailer = parsed.fields.last().expect("trailer field present");
        assert_eq!(trailer.tag, b"86");
        assert!(
            trailer.sequence_path.is_empty(),
            "trailing :86: after the wrapped sequence closes must be at root scope, \
             not attached to the earlier anchored ENTRY occurrence"
        );
    }

    #[test]
    fn empty_anchored_list_leaves_existing_16r_16s_behavior_unchanged() {
        // Sanity: passing no anchored sequences (the default `parse_message`
        // path) behaves identically to a real 16R/16S message — anchors never
        // activate unless declared.
        let message = br"{4:
:16R:GENL
:20C::SEME//ABC123
:16S:GENL
-}";
        let via_default = parse_message(message);
        let via_sequences = parse_message_with_sequences(message, &[mt940_anchor()]);
        assert_eq!(via_default.fields, via_sequences.fields);
    }
}
