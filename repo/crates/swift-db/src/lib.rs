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

//! Database-neutral ingestion contracts and output materialisation.

use std::collections::BTreeMap;
use std::fmt;
use swift_core::{ParsedMessage, SequenceFrame};
use swift_schema::{
    match_and_parse_message, render_payload_column, DatabaseLayout, FieldRuleSchema, MessageSchema,
    ParsedMessageMatch, SchemaCatalog,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundMessage {
    pub id: String,
    pub message_type: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageBatch {
    pub messages: Vec<InboundMessage>,
}

pub trait InboundSource {
    type Error;

    fn read_batch(&mut self, limit: usize) -> Result<MessageBatch, Self::Error>;
    fn mark_processed(&mut self, ids: &[String]) -> Result<(), Self::Error>;
}

pub trait MigrationSink {
    type Error;

    fn apply_layout(&mut self, layout: &DatabaseLayout) -> Result<(), Self::Error>;
}

pub trait ParsedSink {
    type Error;

    fn write_batch(&mut self, batch: &ParsedOutputBatch) -> Result<(), Self::Error>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedOutputBatch {
    pub raw_messages: Vec<RawMessageRow>,
    pub fields: Vec<FieldRow>,
    pub parse_errors: Vec<ParseErrorRow>,
    pub normalized_rows: Vec<NormalizedRow>,
}

impl ParsedOutputBatch {
    pub fn empty() -> Self {
        Self {
            raw_messages: Vec::new(),
            fields: Vec::new(),
            parse_errors: Vec::new(),
            normalized_rows: Vec::new(),
        }
    }

    pub fn extend(&mut self, other: ParsedOutputBatch) {
        self.raw_messages.extend(other.raw_messages);
        self.fields.extend(other.fields);
        self.parse_errors.extend(other.parse_errors);
        self.normalized_rows.extend(other.normalized_rows);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawMessageRow {
    pub message_id: String,
    pub message_type: String,
    pub raw_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldRow {
    pub message_id: String,
    pub sequence_path: Option<String>,
    pub tag: String,
    pub qualifier: Option<String>,
    pub raw_value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseErrorRow {
    pub message_id: String,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedRow {
    pub table: String,
    pub values: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaterializeError {
    SchemaMessageNotFound { message_type: String },
}

impl fmt::Display for MaterializeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SchemaMessageNotFound { message_type } => {
                write!(f, "schema message not found for {message_type}")
            }
        }
    }
}

impl std::error::Error for MaterializeError {}

pub fn materialize_message(
    catalog: &SchemaCatalog,
    inbound: &InboundMessage,
    parsed: &ParsedMessage<'_>,
) -> Result<ParsedOutputBatch, MaterializeError> {
    let schema = catalog.message(&inbound.message_type).ok_or_else(|| {
        MaterializeError::SchemaMessageNotFound {
            message_type: inbound.message_type.clone(),
        }
    })?;
    Ok(materialize_with_schema(catalog, schema, inbound, parsed))
}

pub fn materialize_structural_message(
    inbound: &InboundMessage,
    parsed: &ParsedMessage<'_>,
) -> ParsedOutputBatch {
    let mut batch = ParsedOutputBatch::empty();

    batch.raw_messages.push(RawMessageRow {
        message_id: inbound.id.clone(),
        message_type: inbound.message_type.clone(),
        raw_text: inbound.body.clone(),
    });

    for field in &parsed.fields {
        batch.fields.push(FieldRow {
            message_id: inbound.id.clone(),
            sequence_path: sequence_path_to_string(&field.sequence_path),
            tag: bytes_to_string(field.tag),
            qualifier: field.qualifier.map(bytes_to_string),
            raw_value: bytes_to_string(field.value),
        });
    }

    for diagnostic in &parsed.diagnostics {
        batch.parse_errors.push(ParseErrorRow {
            message_id: inbound.id.clone(),
            error: format!("{diagnostic:?}"),
        });
    }

    batch
}

pub fn materialize_with_schema(
    catalog: &SchemaCatalog,
    schema: &MessageSchema,
    inbound: &InboundMessage,
    parsed: &ParsedMessage<'_>,
) -> ParsedOutputBatch {
    let parsed_match = match_and_parse_message(catalog, schema, parsed);
    let mut batch = ParsedOutputBatch::empty();

    batch.raw_messages.push(RawMessageRow {
        message_id: inbound.id.clone(),
        message_type: inbound.message_type.clone(),
        raw_text: inbound.body.clone(),
    });

    for field in &parsed.fields {
        batch.fields.push(FieldRow {
            message_id: inbound.id.clone(),
            sequence_path: sequence_path_to_string(&field.sequence_path),
            tag: bytes_to_string(field.tag),
            qualifier: field.qualifier.map(bytes_to_string),
            raw_value: bytes_to_string(field.value),
        });
    }

    for diagnostic in &parsed.diagnostics {
        batch.parse_errors.push(ParseErrorRow {
            message_id: inbound.id.clone(),
            error: format!("{diagnostic:?}"),
        });
    }

    append_match_output(&mut batch, &inbound.id, &parsed_match);
    batch
}

fn append_match_output(
    batch: &mut ParsedOutputBatch,
    message_id: &str,
    parsed_match: &ParsedMessageMatch<'_, '_, '_>,
) {
    for missing in &parsed_match.missing_required {
        batch.parse_errors.push(ParseErrorRow {
            message_id: message_id.to_string(),
            error: format!("missing required field {missing:?}"),
        });
    }

    for violation in &parsed_match.cardinality_violations {
        batch.parse_errors.push(ParseErrorRow {
            message_id: message_id.to_string(),
            error: format!("field cardinality violation {violation:?}"),
        });
    }

    for issue in &parsed_match.sequence_issues {
        batch.parse_errors.push(ParseErrorRow {
            message_id: message_id.to_string(),
            error: format!("sequence validation issue {issue:?}"),
        });
    }

    for parse_error in &parsed_match.parse_errors {
        batch.parse_errors.push(ParseErrorRow {
            message_id: message_id.to_string(),
            error: format!(
                "failed to parse field {} using {}: {:?}",
                parse_error.rule.name, parse_error.rule.field_type, parse_error.error
            ),
        });
    }

    for matched in &parsed_match.matched_fields {
        merge_normalized_row(
            &mut batch.normalized_rows,
            normalized_row(
                message_id,
                matched.field.sequence_path.as_slice(),
                matched.rule,
                matched.field_type,
                &matched.captures,
            ),
        );
    }
}

fn merge_normalized_row(rows: &mut Vec<NormalizedRow>, row: NormalizedRow) {
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

fn normalized_row(
    message_id: &str,
    sequence_path: &[SequenceFrame<'_>],
    rule: &FieldRuleSchema,
    field_type: &str,
    captures: &[swift_schema::CapturedFieldValue<'_>],
) -> NormalizedRow {
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

    NormalizedRow {
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
    let (negative, body) = if value.starts_with('N') {
        (true, unsigned)
    } else {
        (false, unsigned)
    };
    let numeric = if body.len() > 3 && body[..3].bytes().all(|byte| byte.is_ascii_uppercase()) {
        &body[3..]
    } else {
        body
    };
    let normalized = numeric.replace(',', ".");
    if negative {
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

fn selected_capture_value(captures: &[swift_schema::CapturedFieldValue<'_>]) -> String {
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

fn bytes_to_string(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use swift_core::parse_message;

    const SCHEMA: &str = r#"
field_types:
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
"#;

    #[test]
    fn materializes_schema_matches_to_output_rows() {
        let catalog = SchemaCatalog::from_yaml_str(SCHEMA).expect("schema loads");
        catalog.validate().expect("schema validates");
        let inbound = InboundMessage {
            id: "msg-1".to_string(),
            message_type: "MT540".to_string(),
            body: "{4:\n:16R:GENL\n:20C::SEME//ABC123\n:16S:GENL\n-}".to_string(),
        };
        let parsed = parse_message(inbound.body.as_bytes());

        let batch = materialize_message(&catalog, &inbound, &parsed).expect("materializes");

        assert_eq!(batch.raw_messages.len(), 1);
        assert_eq!(batch.fields.len(), 3);
        assert!(batch.parse_errors.is_empty());
        assert_eq!(
            batch.normalized_rows,
            vec![NormalizedRow {
                table: "settlement_instruction".to_string(),
                values: BTreeMap::from([
                    ("message_id".to_string(), "msg-1".to_string()),
                    ("sequence_path".to_string(), "GENL[0]".to_string()),
                    ("sender_reference".to_string(), "ABC123".to_string()),
                    (
                        "sender_reference__render_sender_reference".to_string(),
                        "ABC123".to_string(),
                    ),
                ]),
            }]
        );
    }

    #[test]
    fn materializes_repeated_sequences_as_distinct_rows() {
        let schema = r#"
field_types:
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
messages:
  - message: MT537
    sequences:
      STAT: {}
      TRAN:
        parent: STAT
        repeat: true
      TRANSDET:
        parent: TRAN
        repeat: true
    fields:
      - path: STAT/TRAN/TRANSDET
        tag: 20C
        qualifier: RELA
        name: related_reference
        type: reference
        entity: mt537_transaction
        column: related_reference
"#;
        let catalog = SchemaCatalog::from_yaml_str(schema).expect("schema loads");
        catalog.validate().expect("schema validates");
        let inbound = InboundMessage {
            id: "msg-1".to_string(),
            message_type: "MT537".to_string(),
            body: "{4:\n:16R:STAT\n:16R:TRAN\n:16R:TRANSDET\n:20C::RELA//REL1\n:16S:TRANSDET\n:16S:TRAN\n:16R:TRAN\n:16R:TRANSDET\n:20C::RELA//REL2\n:16S:TRANSDET\n:16S:TRAN\n:16S:STAT\n-}".to_string(),
        };
        let parsed = parse_message(inbound.body.as_bytes());

        let batch = materialize_message(&catalog, &inbound, &parsed).expect("materializes");

        assert_eq!(batch.normalized_rows.len(), 2);
        assert_eq!(
            batch.normalized_rows[0].values.get("sequence_path"),
            Some(&"STAT[0]/TRAN[0]/TRANSDET[0]".to_string())
        );
        assert_eq!(
            batch.normalized_rows[0].values.get("related_reference"),
            Some(&"REL1".to_string())
        );
        assert_eq!(
            batch.normalized_rows[0]
                .values
                .get("related_reference__render_related_reference"),
            Some(&"REL1".to_string())
        );
        assert_eq!(
            batch.normalized_rows[1].values.get("sequence_path"),
            Some(&"STAT[0]/TRAN[1]/TRANSDET[0]".to_string())
        );
        assert_eq!(
            batch.normalized_rows[1].values.get("related_reference"),
            Some(&"REL2".to_string())
        );
    }
}
