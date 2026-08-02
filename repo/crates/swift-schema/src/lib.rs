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

//! Schema loading, validation, and generic field matching for SWIFT messages.
//!
//! This crate keeps the authoring format declarative. Later phases can compile
//! these schemas into denser numeric execution plans without changing the user
//! facing YAML shape.

use serde::Deserialize;
use smallvec::SmallVec;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fmt::Write as _;
use swift_core::{FieldSlice, ParsedMessage, SequenceFrame};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaCatalog {
    #[serde(default)]
    pub field_types: Vec<FieldTypeSchema>,
    #[serde(default)]
    pub messages: Vec<MessageSchema>,
}

impl SchemaCatalog {
    pub fn from_yaml_str(input: &str) -> Result<Self, SchemaLoadError> {
        serde_yaml::from_str(input).map_err(SchemaLoadError::Yaml)
    }

    pub fn validate(&self) -> Result<(), SchemaValidationError> {
        let mut errors = Vec::new();
        let mut field_type_names = BTreeSet::new();

        for field_type in &self.field_types {
            if !field_type_names.insert(field_type.name.as_str()) {
                errors.push(SchemaValidationIssue::DuplicateFieldType {
                    name: field_type.name.clone(),
                });
            }
            if field_type.pattern.is_empty() {
                errors.push(SchemaValidationIssue::EmptyFieldTypePattern {
                    name: field_type.name.clone(),
                });
            }
        }

        let mut message_names = BTreeSet::new();
        for message in &self.messages {
            if !message_names.insert(message.message.as_str()) {
                errors.push(SchemaValidationIssue::DuplicateMessage {
                    message: message.message.clone(),
                });
            }
            validate_message(message, &field_type_names, &mut errors);
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(SchemaValidationError { issues: errors })
        }
    }

    pub fn validate_rendering(&self) -> Result<(), SchemaValidationError> {
        self.validate()?;

        let mut errors = Vec::new();
        let field_types = self
            .field_types
            .iter()
            .map(|field_type| (field_type.name.as_str(), field_type))
            .collect::<BTreeMap<_, _>>();

        for message in &self.messages {
            validate_render_mappings(message, &mut errors);
            for field in &message.fields {
                validate_render_metadata(message, field, &field_types, &mut errors);
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(SchemaValidationError { issues: errors })
        }
    }

    pub fn message(&self, message_type: &str) -> Option<&MessageSchema> {
        self.messages
            .iter()
            .find(|message| message.message == message_type)
    }

    pub fn field_type(&self, name: &str) -> Option<&FieldTypeSchema> {
        self.field_types
            .iter()
            .find(|field_type| field_type.name == name)
    }
}

#[derive(Debug)]
pub enum SchemaLoadError {
    Yaml(serde_yaml::Error),
}

impl fmt::Display for SchemaLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Yaml(error) => write!(f, "failed to parse schema YAML: {error}"),
        }
    }
}

impl std::error::Error for SchemaLoadError {}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldTypeSchema {
    pub name: String,
    #[serde(default)]
    pub pattern: Vec<FieldPatternStep>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", deny_unknown_fields)]
pub enum FieldPatternStep {
    Literal {
        value: String,
    },
    Capture {
        name: String,
        value_type: CaptureValueType,
        #[serde(default)]
        length: Option<usize>,
    },
    Rest {
        name: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureValueType {
    AlphaNum,
    Digits,
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageSchema {
    pub message: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub coverage: Option<CoverageTarget>,
    #[serde(default)]
    pub sequences: BTreeMap<String, SequenceSchema>,
    #[serde(default)]
    pub fields: Vec<FieldRuleSchema>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoverageTarget {
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub exact: bool,
    #[serde(default)]
    pub expected_sequences: Option<usize>,
    #[serde(default)]
    pub expected_fields: Option<usize>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SequenceSchema {
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub parents: Vec<String>,
    #[serde(default)]
    pub repeat: bool,
    #[serde(default)]
    pub min: Option<u16>,
    #[serde(default)]
    pub max: Option<u16>,
    /// Marks this as an **anchored** sequence: a repeating field group with NO
    /// `:16R:`/`:16S:` wrapper in the wire format (e.g. MT940's Statement Line
    /// group — tag `61` starts a new occurrence, `86` belongs to it). Root
    /// scope only; mutually exclusive with `parent`/`parents`. See
    /// `swift_core::AnchoredSequence`.
    #[serde(default)]
    pub anchor_tag: Option<String>,
    /// Tags other than `anchor_tag` that belong to the current occurrence
    /// (e.g. `["86"]`). Only meaningful when `anchor_tag` is set.
    #[serde(default)]
    pub member_tags: Vec<String>,
}

/// Extract the schema-declared anchored sequences (see [`SequenceSchema::anchor_tag`])
/// as `swift_core::AnchoredSequence`s, ready to pass to
/// `swift_core::parse_message_with_sequences`. Empty for schemas that only use
/// `:16R:`/`:16S:` sequences (i.e. every schema before MT940).
pub fn anchored_sequences(schema: &MessageSchema) -> Vec<swift_core::AnchoredSequence<'_>> {
    schema
        .sequences
        .iter()
        .filter_map(|(name, seq)| {
            let anchor_tag = seq.anchor_tag.as_deref()?;
            Some(swift_core::AnchoredSequence {
                name: name.as_bytes(),
                anchor_tag: anchor_tag.as_bytes(),
                member_tags: seq.member_tags.iter().map(String::as_bytes).collect(),
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldRuleSchema {
    pub path: String,
    pub tag: String,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub qualifier: Option<String>,
    pub name: String,
    #[serde(rename = "type")]
    pub field_type: String,
    #[serde(default)]
    pub option_types: BTreeMap<String, String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub min: Option<u16>,
    #[serde(default)]
    pub max: Option<u16>,
    pub entity: String,
    pub column: String,
    #[serde(default)]
    pub render: Option<FieldRenderSchema>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldRenderSchema {
    #[serde(default)]
    pub option: Option<String>,
    #[serde(default)]
    pub qualifier: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
}

pub fn render_payload_column(rule: &FieldRuleSchema) -> String {
    format!(
        "{}__render_{}",
        rule.column,
        render_column_suffix(&rule.name)
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaValidationError {
    pub issues: Vec<SchemaValidationIssue>,
}

impl fmt::Display for SchemaValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} schema validation issue(s)", self.issues.len())?;
        for issue in &self.issues {
            write!(f, "\n- {issue:?}")?;
        }
        Ok(())
    }
}

impl std::error::Error for SchemaValidationError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaValidationIssue {
    DuplicateFieldType {
        name: String,
    },
    EmptyFieldTypePattern {
        name: String,
    },
    DuplicateMessage {
        message: String,
    },
    FieldTypeNotFound {
        message: String,
        field: String,
        field_type: String,
    },
    OptionFieldTypeNotFound {
        message: String,
        field: String,
        option: String,
        field_type: String,
    },
    OptionTypeWithoutOptions {
        message: String,
        field: String,
        tag: String,
    },
    SequenceNotFound {
        message: String,
        field: String,
        path: String,
    },
    InvalidSequenceParent {
        message: String,
        sequence: String,
        parent: String,
    },
    EmptyFieldMapping {
        message: String,
        field: String,
    },
    InvalidTagOptions {
        message: String,
        field: String,
        tag: String,
    },
    InvalidFieldCardinality {
        message: String,
        field: String,
        min: Option<u16>,
        max: Option<u16>,
    },
    RenderOptionOnConcreteTag {
        message: String,
        field: String,
        tag: String,
        option: String,
    },
    InvalidRenderOption {
        message: String,
        field: String,
        tag: String,
        option: String,
    },
    MissingRenderOption {
        message: String,
        field: String,
        tag: String,
    },
    MissingRenderQualifier {
        message: String,
        field: String,
        field_type: String,
    },
    DuplicateRenderMapping {
        message: String,
        entity: String,
        path: String,
        column: String,
        fields: Vec<String>,
    },
}

fn validate_message(
    message: &MessageSchema,
    field_type_names: &BTreeSet<&str>,
    errors: &mut Vec<SchemaValidationIssue>,
) {
    for (sequence_name, sequence) in &message.sequences {
        if let Some(parent) = &sequence.parent {
            if !message.sequences.contains_key(parent) {
                errors.push(SchemaValidationIssue::InvalidSequenceParent {
                    message: message.message.clone(),
                    sequence: sequence_name.clone(),
                    parent: parent.clone(),
                });
            }
        }
        for parent in &sequence.parents {
            if parent != "$" && !message.sequences.contains_key(parent) {
                errors.push(SchemaValidationIssue::InvalidSequenceParent {
                    message: message.message.clone(),
                    sequence: sequence_name.clone(),
                    parent: parent.clone(),
                });
            }
        }
    }

    for field in &message.fields {
        if !field_type_names.contains(field.field_type.as_str()) {
            errors.push(SchemaValidationIssue::FieldTypeNotFound {
                message: message.message.clone(),
                field: field.name.clone(),
                field_type: field.field_type.clone(),
            });
        }
        if !field.option_types.is_empty() && field.options.is_empty() {
            errors.push(SchemaValidationIssue::OptionTypeWithoutOptions {
                message: message.message.clone(),
                field: field.name.clone(),
                tag: field.tag.clone(),
            });
        }
        for (option, field_type) in &field.option_types {
            if !field_type_names.contains(field_type.as_str()) {
                errors.push(SchemaValidationIssue::OptionFieldTypeNotFound {
                    message: message.message.clone(),
                    field: field.name.clone(),
                    option: option.clone(),
                    field_type: field_type.clone(),
                });
            }
        }
        if !schema_path_exists(&field.path, message) {
            errors.push(SchemaValidationIssue::SequenceNotFound {
                message: message.message.clone(),
                field: field.name.clone(),
                path: field.path.clone(),
            });
        }
        if field.entity.is_empty() || field.column.is_empty() {
            errors.push(SchemaValidationIssue::EmptyFieldMapping {
                message: message.message.clone(),
                field: field.name.clone(),
            });
        }
        if !field.options.is_empty() && !tag_accepts_options(&field.tag) {
            errors.push(SchemaValidationIssue::InvalidTagOptions {
                message: message.message.clone(),
                field: field.name.clone(),
                tag: field.tag.clone(),
            });
        }
        if matches!((field.min, field.max), (Some(min), Some(max)) if min > max) {
            errors.push(SchemaValidationIssue::InvalidFieldCardinality {
                message: message.message.clone(),
                field: field.name.clone(),
                min: field.min,
                max: field.max,
            });
        }
    }
}

fn validate_render_metadata(
    message: &MessageSchema,
    field: &FieldRuleSchema,
    field_types: &BTreeMap<&str, &FieldTypeSchema>,
    errors: &mut Vec<SchemaValidationIssue>,
) {
    let render_option = field
        .render
        .as_ref()
        .and_then(|render| render.option.as_deref());

    if let Some(option) = render_option {
        if field.options.is_empty() {
            errors.push(SchemaValidationIssue::RenderOptionOnConcreteTag {
                message: message.message.clone(),
                field: field.name.clone(),
                tag: field.tag.clone(),
                option: option.to_string(),
            });
        } else if !field.options.iter().any(|allowed| allowed == option) {
            errors.push(SchemaValidationIssue::InvalidRenderOption {
                message: message.message.clone(),
                field: field.name.clone(),
                tag: field.tag.clone(),
                option: option.to_string(),
            });
        }
    } else if field.options.len() > 1 {
        errors.push(SchemaValidationIssue::MissingRenderOption {
            message: message.message.clone(),
            field: field.name.clone(),
            tag: field.tag.clone(),
        });
    }

    if field.qualifier.is_some()
        || field
            .render
            .as_ref()
            .and_then(|render| render.qualifier.as_ref())
            .is_some()
    {
        return;
    }

    for field_type_name in render_candidate_field_types(field) {
        let Some(field_type) = field_types.get(field_type_name.as_str()) else {
            continue;
        };
        if field_type_uses_capture(field_type, "qualifier") {
            errors.push(SchemaValidationIssue::MissingRenderQualifier {
                message: message.message.clone(),
                field: field.name.clone(),
                field_type: field_type_name,
            });
            break;
        }
    }
}

fn validate_render_mappings(message: &MessageSchema, errors: &mut Vec<SchemaValidationIssue>) {
    let mut mappings = BTreeMap::<(String, String, String), Vec<String>>::new();
    for field in &message.fields {
        mappings
            .entry((
                field.entity.clone(),
                field.path.clone(),
                field.column.clone(),
            ))
            .or_default()
            .push(field.name.clone());
    }

    for ((entity, path, column), fields) in mappings {
        if fields.len() > 1 {
            errors.push(SchemaValidationIssue::DuplicateRenderMapping {
                message: message.message.clone(),
                entity,
                path,
                column,
                fields,
            });
        }
    }
}

fn render_candidate_field_types(field: &FieldRuleSchema) -> Vec<String> {
    if let Some(option) = field
        .render
        .as_ref()
        .and_then(|render| render.option.as_deref())
    {
        return vec![field
            .option_types
            .get(option)
            .cloned()
            .unwrap_or_else(|| field.field_type.clone())];
    }

    if field.options.len() == 1 {
        return vec![field
            .option_types
            .get(&field.options[0])
            .cloned()
            .unwrap_or_else(|| field.field_type.clone())];
    }

    std::iter::once(field.field_type.clone())
        .chain(field.option_types.values().cloned())
        .collect()
}

fn field_type_uses_capture(field_type: &FieldTypeSchema, capture_name: &str) -> bool {
    field_type.pattern.iter().any(|step| match step {
        FieldPatternStep::Capture { name, .. } | FieldPatternStep::Rest { name } => {
            name == capture_name
        }
        FieldPatternStep::Literal { .. } => false,
    })
}

fn schema_path_exists(path: &str, message: &MessageSchema) -> bool {
    path.split('|').all(|path| {
        path == "$"
            || path
                .split('/')
                .all(|sequence| message.sequences.contains_key(sequence))
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchedField<'schema, 'parsed, 'input> {
    pub rule: &'schema FieldRuleSchema,
    pub field: &'parsed FieldSlice<'input>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequiredFieldMiss {
    pub name: String,
    pub path: String,
    pub sequence_path: Option<String>,
    pub tag: String,
    pub qualifier: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardinalityViolation {
    pub name: String,
    pub path: String,
    pub sequence_path: Option<String>,
    pub tag: String,
    pub qualifier: Option<String>,
    pub min: Option<u16>,
    pub max: Option<u16>,
    pub actual: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SequenceValidationIssue {
    UnknownSequence {
        sequence_path: String,
        sequence: String,
    },
    InvalidParent {
        sequence_path: String,
        sequence: String,
        expected_parents: Vec<String>,
        actual_parent: Option<String>,
    },
    Cardinality {
        sequence_path: String,
        sequence: String,
        parent_path: Option<String>,
        min: Option<u16>,
        max: Option<u16>,
        actual: u16,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageMatch<'schema, 'parsed, 'input> {
    pub matched_fields: Vec<MatchedField<'schema, 'parsed, 'input>>,
    pub missing_required: Vec<RequiredFieldMiss>,
    pub cardinality_violations: Vec<CardinalityViolation>,
    pub sequence_issues: Vec<SequenceValidationIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedMatchedField<'schema, 'parsed, 'input> {
    pub rule: &'schema FieldRuleSchema,
    pub field: &'parsed FieldSlice<'input>,
    pub field_type: &'schema str,
    pub captures: Vec<CapturedFieldValue<'input>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedFieldError<'schema, 'parsed, 'input> {
    pub rule: &'schema FieldRuleSchema,
    pub field: &'parsed FieldSlice<'input>,
    pub error: FieldParseError,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedMessageMatch<'schema, 'parsed, 'input> {
    pub matched_fields: Vec<ParsedMatchedField<'schema, 'parsed, 'input>>,
    pub parse_errors: Vec<ParsedFieldError<'schema, 'parsed, 'input>>,
    pub missing_required: Vec<RequiredFieldMiss>,
    pub cardinality_violations: Vec<CardinalityViolation>,
    pub sequence_issues: Vec<SequenceValidationIssue>,
}

pub fn match_message<'schema, 'parsed, 'input>(
    schema: &'schema MessageSchema,
    parsed: &'parsed ParsedMessage<'input>,
) -> MessageMatch<'schema, 'parsed, 'input> {
    let mut matched_fields = Vec::new();
    let mut rule_counts: Vec<BTreeMap<Option<String>, u16>> =
        vec![BTreeMap::new(); schema.fields.len()];
    let mut observed_scopes: Vec<BTreeSet<Option<String>>> =
        vec![BTreeSet::new(); schema.fields.len()];

    for field in &parsed.fields {
        for (rule_index, rule) in schema.fields.iter().enumerate() {
            if path_matches(&rule.path, &field.sequence_path) {
                observed_scopes[rule_index].insert(sequence_path_to_string(&field.sequence_path));
            }
            if rule_matches_field(rule, field) {
                let sequence_path = sequence_path_to_string(&field.sequence_path);
                let count = rule_counts[rule_index].entry(sequence_path).or_default();
                *count = count.saturating_add(1);
                matched_fields.push(MatchedField { rule, field });
            }
        }
    }

    let mut missing_required = Vec::new();
    let mut cardinality_violations = Vec::new();

    for (index, rule) in schema.fields.iter().enumerate() {
        let scopes = cardinality_scopes(
            &observed_scopes[index],
            &rule_counts[index],
            rule.min.or(if rule.required { Some(1) } else { None }),
        );
        for scope in scopes {
            let actual = rule_counts[index].get(&scope).copied().unwrap_or(0);
            if rule.required && actual == 0 {
                missing_required.push(RequiredFieldMiss {
                    name: rule.name.clone(),
                    path: rule.path.clone(),
                    sequence_path: scope.clone(),
                    tag: rule.tag.clone(),
                    qualifier: rule.qualifier.clone(),
                });
            }
            if let Some(violation) = cardinality_violation(rule, scope, actual) {
                cardinality_violations.push(violation);
            }
        }
    }

    MessageMatch {
        matched_fields,
        missing_required,
        cardinality_violations,
        sequence_issues: validate_sequences(schema, parsed),
    }
}

pub fn match_and_parse_message<'schema, 'parsed, 'input>(
    catalog: &'schema SchemaCatalog,
    schema: &'schema MessageSchema,
    parsed: &'parsed ParsedMessage<'input>,
) -> ParsedMessageMatch<'schema, 'parsed, 'input> {
    let matched = match_message(schema, parsed);
    let mut matched_fields = Vec::new();
    let mut parse_errors = Vec::new();

    for matched_field in matched.matched_fields {
        let field_type_name = field_type_for_matched_rule(matched_field.rule, matched_field.field);
        let Some(field_type) = catalog.field_type(field_type_name) else {
            parse_errors.push(ParsedFieldError {
                rule: matched_field.rule,
                field: matched_field.field,
                error: FieldParseError::FieldTypeNotFound {
                    name: field_type_name.to_string(),
                },
            });
            continue;
        };

        match parse_field_value(field_type, matched_field.field.value) {
            Ok(captures) => matched_fields.push(ParsedMatchedField {
                rule: matched_field.rule,
                field: matched_field.field,
                field_type: field_type_name,
                captures,
            }),
            Err(error) => parse_errors.push(ParsedFieldError {
                rule: matched_field.rule,
                field: matched_field.field,
                error,
            }),
        }
    }

    ParsedMessageMatch {
        matched_fields,
        parse_errors,
        missing_required: matched.missing_required,
        cardinality_violations: matched.cardinality_violations,
        sequence_issues: matched.sequence_issues,
    }
}

fn field_type_for_matched_rule<'a>(rule: &'a FieldRuleSchema, field: &FieldSlice<'_>) -> &'a str {
    let option = field
        .tag
        .last()
        .map(|byte| (*byte as char).to_ascii_uppercase().to_string());
    option
        .as_deref()
        .and_then(|option| rule.option_types.get(option))
        .map_or(&rule.field_type, String::as_str)
}

fn cardinality_violation(
    rule: &FieldRuleSchema,
    sequence_path: Option<String>,
    actual: u16,
) -> Option<CardinalityViolation> {
    let min = rule.min.or(if rule.required { Some(1) } else { None });
    let max = rule.max;
    let below_min = min.is_some_and(|min| actual < min);
    let above_max = max.is_some_and(|max| actual > max);

    if below_min || above_max {
        Some(CardinalityViolation {
            name: rule.name.clone(),
            path: rule.path.clone(),
            sequence_path,
            tag: rule.tag.clone(),
            qualifier: rule.qualifier.clone(),
            min,
            max,
            actual,
        })
    } else {
        None
    }
}

fn cardinality_scopes(
    observed: &BTreeSet<Option<String>>,
    counts: &BTreeMap<Option<String>, u16>,
    min: Option<u16>,
) -> BTreeSet<Option<String>> {
    let mut scopes = BTreeSet::new();
    scopes.extend(observed.iter().cloned());
    scopes.extend(counts.keys().cloned());
    if scopes.is_empty() && min.unwrap_or(0) > 0 {
        scopes.insert(None);
    }
    scopes
}

fn validate_sequences(
    schema: &MessageSchema,
    parsed: &ParsedMessage<'_>,
) -> Vec<SequenceValidationIssue> {
    let mut issues = Vec::new();
    let mut seen_paths = BTreeSet::new();
    let mut counts_by_parent: BTreeMap<(Option<String>, String), u16> = BTreeMap::new();

    for field in &parsed.fields {
        for frame_index in 0..field.sequence_path.len() {
            let Some(sequence_path) = sequence_path_to_string(&field.sequence_path[..=frame_index])
            else {
                continue;
            };
            if !seen_paths.insert(sequence_path.clone()) {
                continue;
            }

            let frame = field.sequence_path[frame_index];
            let sequence = String::from_utf8_lossy(frame.name).into_owned();
            let parent_path = if frame_index == 0 {
                None
            } else {
                sequence_path_to_string(&field.sequence_path[..frame_index])
            };
            let actual_parent = if frame_index == 0 {
                None
            } else {
                Some(
                    String::from_utf8_lossy(field.sequence_path[frame_index - 1].name).into_owned(),
                )
            };

            let Some(rule) = schema.sequences.get(&sequence) else {
                issues.push(SequenceValidationIssue::UnknownSequence {
                    sequence_path,
                    sequence,
                });
                continue;
            };

            let allowed_parents = allowed_sequence_parents(rule);
            if !sequence_parent_matches(&allowed_parents, actual_parent.as_deref()) {
                issues.push(SequenceValidationIssue::InvalidParent {
                    sequence_path: sequence_path.clone(),
                    sequence: sequence.clone(),
                    expected_parents: allowed_parents,
                    actual_parent,
                });
            }

            *counts_by_parent.entry((parent_path, sequence)).or_default() += 1;
        }
    }

    for ((parent_path, sequence), actual) in counts_by_parent {
        let Some(rule) = schema.sequences.get(&sequence) else {
            continue;
        };
        let min = rule.min;
        let max = rule.max.or(if rule.repeat { None } else { Some(1) });
        let below_min = min.is_some_and(|min| actual < min);
        let above_max = max.is_some_and(|max| actual > max);

        if below_min || above_max {
            issues.push(SequenceValidationIssue::Cardinality {
                sequence_path: sequence.clone(),
                sequence,
                parent_path,
                min,
                max,
                actual,
            });
        }
    }

    issues
}

fn allowed_sequence_parents(rule: &SequenceSchema) -> Vec<String> {
    let mut parents = rule.parents.clone();
    if let Some(parent) = &rule.parent {
        parents.push(parent.clone());
    }
    parents.sort();
    parents.dedup();
    parents
}

fn sequence_parent_matches(allowed: &[String], actual: Option<&str>) -> bool {
    if allowed.is_empty() {
        actual.is_none()
    } else {
        actual.map_or_else(
            || allowed.iter().any(|allowed| allowed == "$"),
            |actual| allowed.iter().any(|allowed| allowed == actual),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseLayout {
    pub tables: Vec<TableLayout>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableLayout {
    pub name: String,
    pub columns: Vec<ColumnLayout>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnLayout {
    pub name: String,
    pub logical_type: LogicalColumnType,
    pub required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicalColumnType {
    Text,
    Date,
    Decimal,
    Json,
}

#[allow(
    clippy::too_many_lines,
    reason = "layout inference is a single linear pass"
)]
pub fn infer_database_layout(catalog: &SchemaCatalog) -> DatabaseLayout {
    let mut tables: BTreeMap<String, BTreeMap<String, ColumnLayout>> = BTreeMap::new();

    for message in &catalog.messages {
        for field in &message.fields {
            let logical_type = catalog
                .field_type(&field.field_type)
                .map_or(LogicalColumnType::Text, infer_logical_type);
            let columns = tables.entry(field.entity.clone()).or_default();
            columns
                .entry("message_id".to_string())
                .or_insert_with(|| ColumnLayout {
                    name: "message_id".to_string(),
                    logical_type: LogicalColumnType::Text,
                    required: true,
                });
            columns
                .entry("sequence_path".to_string())
                .or_insert_with(|| ColumnLayout {
                    name: "sequence_path".to_string(),
                    logical_type: LogicalColumnType::Text,
                    required: true,
                });
            columns
                .entry(field.column.clone())
                .and_modify(|existing| {
                    existing.required &= field.required;
                    existing.logical_type =
                        merge_logical_types(existing.logical_type, logical_type);
                })
                .or_insert_with(|| ColumnLayout {
                    name: field.column.clone(),
                    logical_type,
                    required: field.required,
                });
            columns
                .entry(render_payload_column(field))
                .or_insert_with(|| ColumnLayout {
                    name: render_payload_column(field),
                    logical_type: LogicalColumnType::Text,
                    required: false,
                });
        }
    }

    let mut layout_tables = vec![
        TableLayout {
            name: "swift_raw_messages".to_string(),
            columns: vec![
                ColumnLayout {
                    name: "message_id".to_string(),
                    logical_type: LogicalColumnType::Text,
                    required: true,
                },
                ColumnLayout {
                    name: "message_type".to_string(),
                    logical_type: LogicalColumnType::Text,
                    required: true,
                },
                ColumnLayout {
                    name: "raw_text".to_string(),
                    logical_type: LogicalColumnType::Text,
                    required: true,
                },
            ],
        },
        TableLayout {
            name: "swift_fields".to_string(),
            columns: vec![
                ColumnLayout {
                    name: "message_id".to_string(),
                    logical_type: LogicalColumnType::Text,
                    required: true,
                },
                ColumnLayout {
                    name: "sequence_path".to_string(),
                    logical_type: LogicalColumnType::Text,
                    required: false,
                },
                ColumnLayout {
                    name: "tag".to_string(),
                    logical_type: LogicalColumnType::Text,
                    required: true,
                },
                ColumnLayout {
                    name: "qualifier".to_string(),
                    logical_type: LogicalColumnType::Text,
                    required: false,
                },
                ColumnLayout {
                    name: "raw_value".to_string(),
                    logical_type: LogicalColumnType::Text,
                    required: true,
                },
            ],
        },
        TableLayout {
            name: "swift_parse_errors".to_string(),
            columns: vec![
                ColumnLayout {
                    name: "message_id".to_string(),
                    logical_type: LogicalColumnType::Text,
                    required: true,
                },
                ColumnLayout {
                    name: "error".to_string(),
                    logical_type: LogicalColumnType::Text,
                    required: true,
                },
            ],
        },
    ];

    layout_tables.extend(tables.into_iter().map(|(name, columns)| TableLayout {
        name,
        columns: columns.into_values().collect(),
    }));

    DatabaseLayout {
        tables: layout_tables,
    }
}

fn infer_logical_type(field_type: &FieldTypeSchema) -> LogicalColumnType {
    let name = field_type.name.as_str();
    if name.contains("amount") || name.contains("quantity") {
        LogicalColumnType::Decimal
    } else if name.contains("datetime") || name.contains("text") {
        LogicalColumnType::Text
    } else if name.contains("date") {
        LogicalColumnType::Date
    } else {
        LogicalColumnType::Text
    }
}

fn merge_logical_types(left: LogicalColumnType, right: LogicalColumnType) -> LogicalColumnType {
    if left == right {
        left
    } else {
        LogicalColumnType::Text
    }
}

fn render_column_suffix(value: &str) -> String {
    let mut suffix = String::new();
    let mut last_was_separator = false;

    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            suffix.push(character.to_ascii_lowercase());
            last_was_separator = false;
        } else if !last_was_separator {
            suffix.push('_');
            last_was_separator = true;
        }
    }

    let suffix = suffix.trim_matches('_');
    if suffix.is_empty() {
        "field".to_string()
    } else {
        suffix.to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedFieldValue<'a> {
    pub name: String,
    pub value: &'a [u8],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldParseError {
    FieldTypeNotFound {
        name: String,
    },
    LiteralMismatch {
        expected: String,
        offset: usize,
    },
    CaptureTooShort {
        name: String,
        expected: usize,
        remaining: usize,
    },
    InvalidCaptureValue {
        name: String,
        value_type: CaptureValueType,
        offset: usize,
    },
    TrailingInput {
        offset: usize,
    },
}

pub fn parse_field_value<'a>(
    field_type: &FieldTypeSchema,
    value: &'a [u8],
) -> Result<Vec<CapturedFieldValue<'a>>, FieldParseError> {
    let mut offset = 0;
    let mut captures = Vec::new();

    for step in &field_type.pattern {
        match step {
            FieldPatternStep::Literal { value: literal } => {
                let literal_bytes = literal.as_bytes();
                if value.get(offset..offset + literal_bytes.len()) == Some(literal_bytes) {
                    offset += literal_bytes.len();
                } else {
                    return Err(FieldParseError::LiteralMismatch {
                        expected: literal.clone(),
                        offset,
                    });
                }
            }
            FieldPatternStep::Capture {
                name,
                value_type,
                length,
            } => {
                let capture_start = offset;
                let capture_end = if let Some(length) = length {
                    let remaining = value.len().saturating_sub(offset);
                    if remaining < *length {
                        return Err(FieldParseError::CaptureTooShort {
                            name: name.clone(),
                            expected: *length,
                            remaining,
                        });
                    }
                    offset + *length
                } else {
                    value.len()
                };
                let captured = &value[capture_start..capture_end];
                if !capture_value_is_valid(captured, *value_type) {
                    return Err(FieldParseError::InvalidCaptureValue {
                        name: name.clone(),
                        value_type: *value_type,
                        offset: capture_start,
                    });
                }
                captures.push(CapturedFieldValue {
                    name: name.clone(),
                    value: captured,
                });
                offset = capture_end;
            }
            FieldPatternStep::Rest { name } => {
                captures.push(CapturedFieldValue {
                    name: name.clone(),
                    value: &value[offset..],
                });
                offset = value.len();
            }
        }
    }

    if offset == value.len() {
        Ok(captures)
    } else {
        Err(FieldParseError::TrailingInput { offset })
    }
}

fn capture_value_is_valid(value: &[u8], value_type: CaptureValueType) -> bool {
    match value_type {
        CaptureValueType::AlphaNum => value.iter().all(u8::is_ascii_alphanumeric),
        CaptureValueType::Digits => value.iter().all(u8::is_ascii_digit),
        CaptureValueType::Text => true,
    }
}

fn rule_matches_field(rule: &FieldRuleSchema, field: &FieldSlice<'_>) -> bool {
    tag_matches(rule, field.tag)
        && qualifier_matches(rule.qualifier.as_deref(), field.qualifier)
        && path_matches(&rule.path, &field.sequence_path)
}

fn tag_matches(rule: &FieldRuleSchema, actual: &[u8]) -> bool {
    if rule.options.is_empty() {
        return rule.tag.as_bytes() == actual;
    }

    let tag = rule.tag.as_bytes();
    if tag.len() != actual.len() || tag.is_empty() {
        return false;
    }
    if !tag[..tag.len() - 1].eq_ignore_ascii_case(&actual[..actual.len() - 1]) {
        return false;
    }

    let Some(actual_option) = actual.last().copied() else {
        return false;
    };

    rule.options.iter().any(|option| {
        option.len() == 1 && option.as_bytes()[0].eq_ignore_ascii_case(&actual_option)
    })
}

fn tag_accepts_options(tag: &str) -> bool {
    tag.as_bytes().last().is_some_and(u8::is_ascii_lowercase)
}

fn qualifier_matches(expected: Option<&str>, actual: Option<&[u8]>) -> bool {
    match (expected, actual) {
        (Some(expected), Some(actual)) => expected
            .split('|')
            .any(|expected| expected.as_bytes() == actual),
        (Some(_), None) => false,
        (None, _) => true,
    }
}

fn path_matches(expected: &str, actual: &[SequenceFrame<'_>]) -> bool {
    if expected.contains('|') {
        return expected
            .split('|')
            .any(|expected| path_matches(expected, actual));
    }

    if expected == "$" {
        return actual.is_empty();
    }

    let expected_parts = expected.split('/');
    if expected_parts.clone().count() != actual.len() {
        return false;
    }

    expected_parts
        .zip(actual)
        .all(|(expected, actual)| expected.as_bytes() == actual.name)
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderEnvelope {
    pub block1: String,
    pub block2: String,
    pub block3: Option<String>,
    pub block5: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderRow {
    pub table: String,
    pub values: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderRequest {
    pub message_id: String,
    pub message_type: String,
    pub envelope: RenderEnvelope,
    pub rows: Vec<RenderRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderError {
    MessageSchemaNotFound {
        message_type: String,
    },
    FieldTypeNotFound {
        field: String,
        field_type: String,
    },
    MissingRenderOption {
        field: String,
        tag: String,
    },
    InvalidRenderOption {
        field: String,
        tag: String,
        option: String,
    },
    MissingRenderQualifier {
        field: String,
    },
    MissingRequiredValue {
        field: String,
        table: String,
        column: String,
    },
    CardinalityViolation {
        field: String,
        table: String,
        column: String,
        sequence_path: String,
        min: Option<u16>,
        max: Option<u16>,
        actual: usize,
    },
    SequenceValidation {
        issue: SequenceValidationIssue,
    },
    InvalidFieldPattern {
        field: String,
        reason: String,
    },
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MessageSchemaNotFound { message_type } => {
                write!(f, "message schema not found for {message_type}")
            }
            Self::FieldTypeNotFound { field, field_type } => {
                write!(
                    f,
                    "field {field} references unknown field type {field_type}"
                )
            }
            Self::MissingRenderOption { field, tag } => {
                write!(
                    f,
                    "field {field} with generic tag {tag} needs render.option"
                )
            }
            Self::InvalidRenderOption { field, tag, option } => {
                write!(f, "field {field} tag {tag} cannot render option {option}")
            }
            Self::MissingRenderQualifier { field } => {
                write!(f, "field {field} needs a qualifier for rendering")
            }
            Self::MissingRequiredValue {
                field,
                table,
                column,
            } => {
                write!(
                    f,
                    "missing required render value for field {field} at {table}.{column}"
                )
            }
            Self::CardinalityViolation {
                field,
                table,
                column,
                sequence_path,
                min,
                max,
                actual,
            } => {
                write!(
                    f,
                    "render cardinality violation for field {field} at {table}.{column} in {sequence_path}: actual {actual}, min {min:?}, max {max:?}"
                )
            }
            Self::SequenceValidation { issue } => {
                write!(f, "render sequence validation failed: {issue:?}")
            }
            Self::InvalidFieldPattern { field, reason } => {
                write!(f, "field {field} cannot be rendered: {reason}")
            }
        }
    }
}

impl std::error::Error for RenderError {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RenderedField {
    schema_index: usize,
    path: String,
    sequence_path: String,
    tag: String,
    value: String,
}

pub fn render_message(
    catalog: &SchemaCatalog,
    request: &RenderRequest,
) -> Result<String, RenderError> {
    let schema = catalog.message(&request.message_type).ok_or_else(|| {
        RenderError::MessageSchemaNotFound {
            message_type: request.message_type.clone(),
        }
    })?;
    let fields = render_block4_fields(catalog, schema, request)?;
    let block4 = render_block4(schema, &fields);

    let mut output = String::new();
    push_fmt(
        &mut output,
        format_args!("{{1:{}}}", request.envelope.block1),
    );
    push_fmt(
        &mut output,
        format_args!("{{2:{}}}", request.envelope.block2),
    );
    if let Some(block3) = &request.envelope.block3 {
        push_fmt(&mut output, format_args!("{{3:{block3}}}"));
    }
    output.push_str("{4:\n");
    output.push_str(&block4);
    output.push_str("-}");
    if let Some(block5) = &request.envelope.block5 {
        push_fmt(&mut output, format_args!("{{5:{block5}}}"));
    }
    Ok(output)
}

fn render_block4_fields(
    catalog: &SchemaCatalog,
    schema: &MessageSchema,
    request: &RenderRequest,
) -> Result<Vec<RenderedField>, RenderError> {
    let mut rendered = Vec::new();
    let sequence_order = render_sequence_order(schema);

    for (schema_index, rule) in schema.fields.iter().enumerate() {
        let matching_rows = request
            .rows
            .iter()
            .filter(|row| row.table == rule.entity)
            .filter(|row| row.values.get("message_id") == Some(&request.message_id))
            .filter(|row| {
                row.values
                    .get("sequence_path")
                    .map_or(rule.path == "$", |path| {
                        path_matches_schema_path(&rule.path, path)
                    })
            })
            .filter(|row| row.values.contains_key(&rule.column))
            .collect::<SmallVec<[_; 4]>>();

        if matching_rows.is_empty() {
            if rule.required || rule.min.unwrap_or(0) > 0 {
                return Err(RenderError::MissingRequiredValue {
                    field: rule.name.clone(),
                    table: rule.entity.clone(),
                    column: rule.column.clone(),
                });
            }
            continue;
        }
        validate_render_cardinality(rule, &matching_rows)?;

        let tag = render_tag(rule)?;
        let field_type_name = render_field_type(rule)?;
        let field_type =
            catalog
                .field_type(field_type_name)
                .ok_or_else(|| RenderError::FieldTypeNotFound {
                    field: rule.name.clone(),
                    field_type: field_type_name.to_string(),
                })?;

        for row in matching_rows {
            let render_column = render_payload_column(rule);
            let value =
                row.values
                    .get(&rule.column)
                    .ok_or_else(|| RenderError::MissingRequiredValue {
                        field: rule.name.clone(),
                        table: rule.entity.clone(),
                        column: rule.column.clone(),
                    })?;
            let payload = row.values.get(&render_column).map(String::as_str);
            let raw_value = render_field_value(rule, field_type, field_type_name, value, payload)?;
            rendered.push(RenderedField {
                schema_index,
                path: primary_schema_path(&rule.path).to_string(),
                sequence_path: row
                    .values
                    .get("sequence_path")
                    .cloned()
                    .unwrap_or_else(|| "$".to_string()),
                tag: tag.clone(),
                value: raw_value,
            });
        }
    }

    rendered.sort_by_key(|field| render_field_order_key(field, &sequence_order));
    validate_render_sequences(schema, &rendered)?;
    Ok(rendered)
}

fn render_sequence_order(schema: &MessageSchema) -> BTreeMap<String, usize> {
    let mut order = BTreeMap::new();
    for (field_index, field) in schema.fields.iter().enumerate() {
        let mut prefix = SmallVec::<[&str; 8]>::new();
        for sequence in primary_schema_path(&field.path)
            .split('/')
            .filter(|sequence| *sequence != "$")
        {
            prefix.push(sequence);
            order.entry(prefix.join("/")).or_insert(field_index);
        }
    }
    order
}

fn validate_render_cardinality(
    rule: &FieldRuleSchema,
    rows: &[&RenderRow],
) -> Result<(), RenderError> {
    if rule.min.is_none() && rule.max.is_none() {
        return Ok(());
    }

    let mut counts = BTreeMap::<String, usize>::new();
    for row in rows {
        let sequence_path = row
            .values
            .get("sequence_path")
            .cloned()
            .unwrap_or_else(|| "$".to_string());
        *counts.entry(sequence_path).or_default() += 1;
    }

    for (sequence_path, actual) in counts {
        let below_min = rule.min.is_some_and(|min| actual < usize::from(min));
        let above_max = rule.max.is_some_and(|max| actual > usize::from(max));
        if below_min || above_max {
            return Err(RenderError::CardinalityViolation {
                field: rule.name.clone(),
                table: rule.entity.clone(),
                column: rule.column.clone(),
                sequence_path,
                min: rule.min,
                max: rule.max,
                actual,
            });
        }
    }

    Ok(())
}

fn validate_render_sequences(
    schema: &MessageSchema,
    fields: &[RenderedField],
) -> Result<(), RenderError> {
    let mut seen_paths = BTreeSet::new();
    let mut counts_by_parent = BTreeMap::<(Option<String>, String), u16>::new();

    for field in fields {
        let parts = concrete_sequence_parts(&field.sequence_path);
        for part_index in 0..parts.len() {
            let sequence_path = concrete_sequence_path(&parts[..=part_index]);
            if !seen_paths.insert(sequence_path.clone()) {
                continue;
            }

            let sequence = parts[part_index].name.clone();
            let parent_path = if part_index == 0 {
                None
            } else {
                Some(concrete_sequence_path(&parts[..part_index]))
            };
            let actual_parent = if part_index == 0 {
                None
            } else {
                Some(parts[part_index - 1].name.clone())
            };

            let Some(rule) = schema.sequences.get(&sequence) else {
                return Err(RenderError::SequenceValidation {
                    issue: SequenceValidationIssue::UnknownSequence {
                        sequence_path,
                        sequence,
                    },
                });
            };

            let allowed_parents = allowed_sequence_parents(rule);
            if !sequence_parent_matches(&allowed_parents, actual_parent.as_deref()) {
                return Err(RenderError::SequenceValidation {
                    issue: SequenceValidationIssue::InvalidParent {
                        sequence_path: sequence_path.clone(),
                        sequence: sequence.clone(),
                        expected_parents: allowed_parents,
                        actual_parent,
                    },
                });
            }

            *counts_by_parent.entry((parent_path, sequence)).or_default() += 1;
        }
    }

    for ((parent_path, sequence), actual) in counts_by_parent {
        let Some(rule) = schema.sequences.get(&sequence) else {
            continue;
        };
        let min = rule.min;
        let max = rule.max.or(if rule.repeat { None } else { Some(1) });
        let below_min = min.is_some_and(|min| actual < min);
        let above_max = max.is_some_and(|max| actual > max);

        if below_min || above_max {
            return Err(RenderError::SequenceValidation {
                issue: SequenceValidationIssue::Cardinality {
                    sequence_path: sequence.clone(),
                    sequence,
                    parent_path,
                    min,
                    max,
                    actual,
                },
            });
        }
    }

    Ok(())
}

fn render_field_order_key(
    field: &RenderedField,
    sequence_order: &BTreeMap<String, usize>,
) -> SmallVec<[(usize, u16); 8]> {
    let mut key = SmallVec::new();
    let mut prefix = SmallVec::<[String; 8]>::new();

    for segment in concrete_sequence_parts(&field.sequence_path) {
        prefix.push(segment.name);
        key.push((
            sequence_order
                .get(&prefix.join("/"))
                .copied()
                .unwrap_or(usize::MAX),
            segment.occurrence,
        ));
    }

    key.push((field.schema_index, 0));
    key
}

/// Anchored sequences (see [`SequenceSchema::anchor_tag`]) have no `:16R:`/
/// `:16S:` wrapper in the real wire format — the wrapper is implicit (a
/// repeating anchor tag), not an explicit envelope. `render_block4` still uses
/// their occurrence boundaries to order/group fields (via `concrete_sequence_parts`),
/// it just must not EMIT the wrapper lines for them, or the round-trip render
/// would inject markers that were never in the original message.
fn is_anchored_sequence(schema: &MessageSchema, name: &str) -> bool {
    schema
        .sequences
        .get(name)
        .is_some_and(|sequence| sequence.anchor_tag.is_some())
}

fn render_block4(schema: &MessageSchema, fields: &[RenderedField]) -> String {
    let mut output = String::new();
    let mut open_path = SmallVec::<[ConcreteSequencePart; 8]>::new();

    for field in fields {
        let target_path = concrete_sequence_parts(&field.sequence_path);
        let common = common_prefix_len(&open_path, &target_path);
        for sequence in open_path[common..].iter().rev() {
            if !is_anchored_sequence(schema, &sequence.name) {
                push_fmt(&mut output, format_args!(":16S:{}\n", sequence.name));
            }
        }
        for sequence in &target_path[common..] {
            if !is_anchored_sequence(schema, &sequence.name) {
                push_fmt(&mut output, format_args!(":16R:{}\n", sequence.name));
            }
        }
        open_path = target_path;
        push_fmt(
            &mut output,
            format_args!(":{}:{}\n", field.tag, field.value),
        );
    }

    for sequence in open_path.iter().rev() {
        if !is_anchored_sequence(schema, &sequence.name) {
            push_fmt(&mut output, format_args!(":16S:{}\n", sequence.name));
        }
    }

    output
}

fn push_fmt(output: &mut String, args: fmt::Arguments<'_>) {
    if output.write_fmt(args).is_err() {
        unreachable!("writing formatted data to String is infallible");
    }
}

fn render_tag(rule: &FieldRuleSchema) -> Result<String, RenderError> {
    if rule.options.is_empty() {
        return Ok(rule.tag.clone());
    }

    let option = render_option(rule)?;

    if option.len() != 1 || !rule.options.iter().any(|allowed| allowed == option) {
        return Err(RenderError::InvalidRenderOption {
            field: rule.name.clone(),
            tag: rule.tag.clone(),
            option: option.to_string(),
        });
    }

    let mut tag = rule.tag.clone();
    tag.pop();
    tag.push_str(option);
    Ok(tag)
}

fn render_field_type(rule: &FieldRuleSchema) -> Result<&str, RenderError> {
    let Some(option) = render_optional_option(rule)? else {
        return Ok(&rule.field_type);
    };

    Ok(rule
        .option_types
        .get(option)
        .map_or(&rule.field_type, String::as_str))
}

fn render_optional_option(rule: &FieldRuleSchema) -> Result<Option<&str>, RenderError> {
    if rule.options.is_empty() {
        return Ok(None);
    }

    Ok(Some(render_option(rule)?))
}

fn render_option(rule: &FieldRuleSchema) -> Result<&str, RenderError> {
    if let Some(option) = rule
        .render
        .as_ref()
        .and_then(|render| render.option.as_deref())
    {
        Ok(option)
    } else if rule.options.len() == 1 {
        Ok(rule.options[0].as_str())
    } else {
        Err(RenderError::MissingRenderOption {
            field: rule.name.clone(),
            tag: rule.tag.clone(),
        })
    }
}

fn render_field_value(
    rule: &FieldRuleSchema,
    field_type: &FieldTypeSchema,
    field_type_name: &str,
    normalized_value: &str,
    render_payload: Option<&str>,
) -> Result<String, RenderError> {
    let mut output = String::new();
    for step in &field_type.pattern {
        match step {
            FieldPatternStep::Literal { value } => output.push_str(value),
            FieldPatternStep::Capture { name, .. } | FieldPatternStep::Rest { name } => {
                output.push_str(&render_capture(
                    rule,
                    field_type_name,
                    name,
                    normalized_value,
                    render_payload,
                )?);
            }
        }
    }
    Ok(output)
}

fn render_capture(
    rule: &FieldRuleSchema,
    field_type_name: &str,
    capture_name: &str,
    normalized_value: &str,
    render_payload: Option<&str>,
) -> Result<String, RenderError> {
    match capture_name {
        "qualifier" => rule
            .render
            .as_ref()
            .and_then(|render| render.qualifier.as_ref())
            .or(rule.qualifier.as_ref())
            .cloned()
            .ok_or_else(|| RenderError::MissingRenderQualifier {
                field: rule.name.clone(),
            }),
        "date" => Ok(render_date_like(normalized_value, render_payload)),
        "amount" => Ok(render_amount_like(normalized_value, render_payload)),
        "quantity" => Ok(render_quantity_like(normalized_value, render_payload)),
        "value" | "code" => Ok(render_value_by_field_type(
            field_type_name,
            normalized_value,
            render_payload,
        )),
        other => Err(RenderError::InvalidFieldPattern {
            field: rule.name.clone(),
            reason: format!("unsupported capture '{other}'"),
        }),
    }
}

fn render_value_by_field_type(
    field_type_name: &str,
    value: &str,
    render_payload: Option<&str>,
) -> String {
    if field_type_name.contains("amount") {
        render_amount_like(value, render_payload)
    } else if field_type_name.contains("quantity") {
        render_quantity_like(value, render_payload)
    } else if field_type_name.contains("date") {
        render_date_like(value, render_payload)
    } else {
        render_text_like(value, render_payload)
    }
}

fn render_text_like(value: &str, render_payload: Option<&str>) -> String {
    match render_payload {
        Some(payload) if payload == value => payload.to_string(),
        _ => value.to_string(),
    }
}

fn render_date_like(value: &str, render_payload: Option<&str>) -> String {
    let rendered = value.replace('-', "");
    if render_payload == Some(rendered.as_str()) {
        render_payload.unwrap_or(rendered.as_str()).to_string()
    } else {
        rendered
    }
}

fn render_amount_like(value: &str, render_payload: Option<&str>) -> String {
    if let Some(payload) = render_payload {
        if decimal_payload_matches(value, amount_numeric_payload(payload)) {
            return payload.to_string();
        }
    }

    let negative = value.trim().starts_with('-');
    let numeric = value
        .trim()
        .strip_prefix('-')
        .unwrap_or_else(|| value.trim());
    let currency = render_payload
        .and_then(amount_currency_prefix)
        .unwrap_or_default();
    format!(
        "{}{}{}",
        if negative { "N" } else { "" },
        currency,
        render_decimal_like(numeric)
    )
}

fn render_quantity_like(value: &str, render_payload: Option<&str>) -> String {
    if let Some(payload) = render_payload {
        if decimal_payload_matches(value, quantity_numeric_payload(payload)) {
            return payload.to_string();
        }
    }

    let prefix = render_payload
        .and_then(|payload| {
            payload
                .rsplit_once('/')
                .map(|(prefix, _)| format!("{prefix}/"))
        })
        .unwrap_or_default();
    format!("{}{}", prefix, render_decimal_like(value))
}

fn amount_currency_prefix(payload: &str) -> Option<&str> {
    let body = payload.strip_prefix('N').unwrap_or(payload);
    if body.len() >= 3 && body.as_bytes()[..3].iter().all(u8::is_ascii_uppercase) {
        Some(&body[..3])
    } else {
        None
    }
}

fn amount_numeric_payload(payload: &str) -> &str {
    let body = payload.strip_prefix('N').unwrap_or(payload);
    if body.len() >= 3 && body.as_bytes()[..3].iter().all(u8::is_ascii_uppercase) {
        &body[3..]
    } else {
        body
    }
}

fn quantity_numeric_payload(payload: &str) -> &str {
    payload.rsplit_once('/').map_or(payload, |(_, value)| value)
}

fn decimal_payload_matches(normalized: &str, payload: &str) -> bool {
    canonical_decimal(normalized) == canonical_decimal(payload)
}

fn canonical_decimal(value: &str) -> String {
    let mut value = value.trim().replace(',', ".");
    let negative = value.starts_with('-') || value.starts_with('N');
    if value.starts_with('-') || value.starts_with('N') {
        value.remove(0);
    }
    if let Some(dot) = value.find('.') {
        while value.len() > dot + 1 && value.ends_with('0') {
            value.pop();
        }
        if value.ends_with('.') {
            value.pop();
        }
    }
    if value.is_empty() {
        value.push('0');
    }
    if negative && value != "0" {
        format!("-{value}")
    } else {
        value
    }
}

fn render_decimal_like(value: &str) -> String {
    let mut value = value.trim().replace('.', ",");
    while value.contains(',') && value.ends_with('0') {
        value.pop();
    }
    if value.ends_with(',') {
        value.push('0');
    }
    value
}

fn path_matches_schema_path(schema_path: &str, sequence_path: &str) -> bool {
    if schema_path.contains('|') {
        return schema_path
            .split('|')
            .any(|schema_path| path_matches_schema_path(schema_path, sequence_path));
    }

    if schema_path == "$" {
        return sequence_path == "$";
    }
    concrete_sequence_names(sequence_path).join("/") == schema_path
}

fn primary_schema_path(schema_path: &str) -> &str {
    schema_path
        .split_once('|')
        .map_or(schema_path, |(path, _)| path)
}

fn concrete_sequence_names(sequence_path: &str) -> Vec<String> {
    if sequence_path == "$" || sequence_path.is_empty() {
        return Vec::new();
    }

    sequence_path
        .split('/')
        .map(|part| {
            part.split_once('[')
                .map_or(part, |(name, _)| name)
                .to_string()
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConcreteSequencePart {
    name: String,
    occurrence: u16,
}

fn concrete_sequence_parts(sequence_path: &str) -> SmallVec<[ConcreteSequencePart; 8]> {
    if sequence_path == "$" || sequence_path.is_empty() {
        return SmallVec::new();
    }

    sequence_path
        .split('/')
        .map(|part| {
            let Some((name, occurrence)) = part.split_once('[') else {
                return ConcreteSequencePart {
                    name: part.to_string(),
                    occurrence: 0,
                };
            };
            ConcreteSequencePart {
                name: name.to_string(),
                occurrence: occurrence.trim_end_matches(']').parse().unwrap_or(0),
            }
        })
        .collect()
}

fn concrete_sequence_path(parts: &[ConcreteSequencePart]) -> String {
    parts
        .iter()
        .map(|part| format!("{}[{}]", part.name, part.occurrence))
        .collect::<Vec<_>>()
        .join("/")
}

fn common_prefix_len<T: PartialEq>(left: &[T], right: &[T]) -> usize {
    left.iter()
        .zip(right)
        .take_while(|(left, right)| left == right)
        .count()
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::needless_raw_string_hashes,
        reason = "snapshot and fixture literals are kept visually stable"
    )]

    use super::*;
    use expect_test::expect;
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
  - name: date_yyyymmdd
    pattern:
      - kind: literal
        value: ":"
      - kind: capture
        name: qualifier
        value_type: alpha_num
        length: 4
      - kind: literal
        value: "//"
      - kind: capture
        name: date
        value_type: digits
        length: 8
messages:
  - message: MT540
    category: securities
    version: "2026"
    sequences:
      GENL:
        repeat: false
      LINK:
        parent: GENL
        repeat: true
    fields:
      - path: GENL
        tag: 20C
        qualifier: SEME
        name: sender_reference
        type: reference
        required: true
        entity: settlement_instruction
        column: sender_reference
      - path: GENL
        tag: 98A
        qualifier: PREP
        name: preparation_date
        type: date_yyyymmdd
        entity: settlement_instruction
        column: preparation_date
      - path: GENL/LINK
        tag: 20C
        qualifier: RELA
        name: related_reference
        type: reference
        entity: message_reference
        column: related_reference
"#;

    fn render_envelope() -> RenderEnvelope {
        RenderEnvelope {
            block1: "F01BANKBEBBAXXX0000000000".to_string(),
            block2: "I540BANKDEFFXXXXN".to_string(),
            block3: None,
            block5: None,
        }
    }

    fn render_row(table: &str, sequence_path: &str, column: &str, value: &str) -> RenderRow {
        RenderRow {
            table: table.to_string(),
            values: BTreeMap::from([
                ("message_id".to_string(), "msg-1".to_string()),
                ("sequence_path".to_string(), sequence_path.to_string()),
                (column.to_string(), value.to_string()),
            ]),
        }
    }

    #[test]
    fn loads_and_validates_schema() {
        let catalog = SchemaCatalog::from_yaml_str(SCHEMA).expect("schema loads");

        catalog.validate().expect("schema validates");
        assert_eq!(catalog.field_types.len(), 2);
        assert_eq!(
            catalog.message("MT540").map(|schema| schema.fields.len()),
            Some(3)
        );
    }

    #[test]
    fn renders_canonical_fin_message_from_normalized_rows() {
        let catalog = SchemaCatalog::from_yaml_str(SCHEMA).expect("schema loads");
        let rendered = render_message(
            &catalog,
            &RenderRequest {
                message_id: "msg-1".to_string(),
                message_type: "MT540".to_string(),
                envelope: render_envelope(),
                rows: vec![
                    render_row(
                        "settlement_instruction",
                        "GENL[1]",
                        "sender_reference",
                        "ABC123",
                    ),
                    render_row(
                        "settlement_instruction",
                        "GENL[1]",
                        "preparation_date",
                        "2026-05-11",
                    ),
                    render_row(
                        "message_reference",
                        "GENL[1]/LINK[1]",
                        "related_reference",
                        "REL1",
                    ),
                ],
            },
        )
        .expect("message renders");

        expect![[r#"
            {1:F01BANKBEBBAXXX0000000000}{2:I540BANKDEFFXXXXN}{4:
            :16R:GENL
            :20C::SEME//ABC123
            :98A::PREP//20260511
            :16R:LINK
            :20C::RELA//REL1
            :16S:LINK
            :16S:GENL
            -}"#]]
        .assert_eq(&rendered);
    }

    #[test]
    fn renders_repeated_sequence_occurrences_without_interleaving_fields() {
        let catalog = SchemaCatalog::from_yaml_str(
            r#"
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
  - name: status
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
        name: code
messages:
  - message: MT537
    sequences:
      STAT: {}
      TRAN:
        parent: STAT
        repeat: true
    fields:
      - path: STAT/TRAN
        tag: 20C
        qualifier: RELA
        name: related_reference
        type: reference
        entity: transaction
        column: related_reference
      - path: STAT/TRAN
        tag: 25D
        qualifier: SETT
        name: settlement_status
        type: status
        entity: transaction
        column: status
"#,
        )
        .expect("schema loads");

        let rendered = render_message(
            &catalog,
            &RenderRequest {
                message_id: "msg-1".to_string(),
                message_type: "MT537".to_string(),
                envelope: render_envelope(),
                rows: vec![
                    render_row(
                        "transaction",
                        "STAT[0]/TRAN[0]",
                        "related_reference",
                        "REL1",
                    ),
                    render_row(
                        "transaction",
                        "STAT[0]/TRAN[1]",
                        "related_reference",
                        "REL2",
                    ),
                    render_row("transaction", "STAT[0]/TRAN[0]", "status", "PEND"),
                    render_row("transaction", "STAT[0]/TRAN[1]", "status", "SETT"),
                ],
            },
        )
        .expect("message renders");

        expect![[r#"
            {1:F01BANKBEBBAXXX0000000000}{2:I540BANKDEFFXXXXN}{4:
            :16R:STAT
            :16R:TRAN
            :20C::RELA//REL1
            :25D::SETT//PEND
            :16S:TRAN
            :16R:TRAN
            :20C::RELA//REL2
            :25D::SETT//SETT
            :16S:TRAN
            :16S:STAT
            -}"#]]
        .assert_eq(&rendered);
    }

    #[test]
    fn render_reports_generic_tag_without_option_metadata() {
        let catalog = SchemaCatalog::from_yaml_str(
            r#"
field_types:
  - name: date_yyyymmdd
    pattern:
      - kind: literal
        value: ":"
      - kind: capture
        name: qualifier
        value_type: alpha_num
        length: 4
      - kind: literal
        value: "//"
      - kind: capture
        name: date
        value_type: digits
        length: 8
messages:
  - message: MT540
    sequences:
      GENL: {}
    fields:
      - path: GENL
        tag: 98a
        options: [A, B]
        qualifier: PREP
        name: preparation_date
        type: date_yyyymmdd
        required: true
        entity: settlement_instruction
        column: preparation_date
"#,
        )
        .expect("schema loads");

        let error = render_message(
            &catalog,
            &RenderRequest {
                message_id: "msg-1".to_string(),
                message_type: "MT540".to_string(),
                envelope: render_envelope(),
                rows: vec![render_row(
                    "settlement_instruction",
                    "GENL[1]",
                    "preparation_date",
                    "2026-05-11",
                )],
            },
        )
        .expect_err("render should fail without option");

        assert_eq!(
            error,
            RenderError::MissingRenderOption {
                field: "preparation_date".to_string(),
                tag: "98a".to_string(),
            }
        );
    }

    #[test]
    fn renders_generic_tag_with_option_metadata() {
        let catalog = SchemaCatalog::from_yaml_str(
            r#"
field_types:
  - name: date_yyyymmdd
    pattern:
      - kind: literal
        value: ":"
      - kind: capture
        name: qualifier
        value_type: alpha_num
        length: 4
      - kind: literal
        value: "//"
      - kind: capture
        name: date
        value_type: digits
        length: 8
messages:
  - message: MT540
    sequences:
      GENL: {}
    fields:
      - path: GENL
        tag: 98a
        options: [A, B]
        qualifier: PREP
        name: preparation_date
        type: date_yyyymmdd
        required: true
        entity: settlement_instruction
        column: preparation_date
        render:
          option: A
"#,
        )
        .expect("schema loads");

        let rendered = render_message(
            &catalog,
            &RenderRequest {
                message_id: "msg-1".to_string(),
                message_type: "MT540".to_string(),
                envelope: render_envelope(),
                rows: vec![render_row(
                    "settlement_instruction",
                    "GENL[1]",
                    "preparation_date",
                    "2026-05-11",
                )],
            },
        )
        .expect("render should use option");

        assert!(rendered.contains(":98A::PREP//20260511\n"));
    }

    #[test]
    fn renders_single_option_generic_tag_with_option_specific_type() {
        let catalog = SchemaCatalog::from_yaml_str(
            r#"
field_types:
  - name: raw_text
    pattern:
      - kind: rest
        name: value
  - name: date_yyyymmdd
    pattern:
      - kind: literal
        value: ":"
      - kind: capture
        name: qualifier
        value_type: alpha_num
        length: 4
      - kind: literal
        value: "//"
      - kind: capture
        name: date
        value_type: digits
        length: 8
messages:
  - message: MT540
    sequences:
      GENL: {}
    fields:
      - path: GENL
        tag: 98a
        options: [A]
        qualifier: PREP
        name: preparation_date
        type: raw_text
        option_types:
          A: date_yyyymmdd
        required: true
        entity: settlement_instruction
        column: preparation_date
"#,
        )
        .expect("schema loads");

        let rendered = render_message(
            &catalog,
            &RenderRequest {
                message_id: "msg-1".to_string(),
                message_type: "MT540".to_string(),
                envelope: render_envelope(),
                rows: vec![render_row(
                    "settlement_instruction",
                    "GENL[1]",
                    "preparation_date",
                    "2026-05-11",
                )],
            },
        )
        .expect("render should infer the only option and its field type");

        assert!(rendered.contains(":98A::PREP//20260511\n"));
    }

    #[test]
    fn render_payload_preserves_format_but_allows_normalized_edits() {
        let catalog = SchemaCatalog::from_yaml_str(
            r#"
field_types:
  - name: quantity
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
        name: quantity
  - name: amount
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
        name: amount
messages:
  - message: MT540
    sequences:
      FIAC: {}
      AMT: {}
    fields:
      - path: FIAC
        tag: 36B
        qualifier: SETT
        name: settlement_quantity
        type: quantity
        entity: settlement_quantity
        column: quantity
      - path: AMT
        tag: 19A
        qualifier: SETT
        name: settlement_amount
        type: amount
        entity: settlement_amount
        column: amount
"#,
        )
        .expect("schema loads");

        let rendered = render_message(
            &catalog,
            &RenderRequest {
                message_id: "msg-1".to_string(),
                message_type: "MT540".to_string(),
                envelope: render_envelope(),
                rows: vec![
                    RenderRow {
                        table: "settlement_quantity".to_string(),
                        values: BTreeMap::from([
                            ("message_id".to_string(), "msg-1".to_string()),
                            ("sequence_path".to_string(), "FIAC[0]".to_string()),
                            ("quantity".to_string(), "2000.000000000000".to_string()),
                            (
                                "quantity__render_settlement_quantity".to_string(),
                                "UNIT/1000,".to_string(),
                            ),
                        ]),
                    },
                    RenderRow {
                        table: "settlement_amount".to_string(),
                        values: BTreeMap::from([
                            ("message_id".to_string(), "msg-1".to_string()),
                            ("sequence_path".to_string(), "AMT[0]".to_string()),
                            ("amount".to_string(), "987.650000000000".to_string()),
                            (
                                "amount__render_settlement_amount".to_string(),
                                "GBP123,45".to_string(),
                            ),
                        ]),
                    },
                ],
            },
        )
        .expect("message renders");

        assert!(rendered.contains(":36B::SETT//UNIT/2000,0\n"));
        assert!(rendered.contains(":19A::SETT//GBP987,65\n"));
    }

    #[test]
    fn render_payload_preserves_text_only_when_normalized_value_is_unchanged() {
        let catalog = SchemaCatalog::from_yaml_str(
            r#"
field_types:
  - name: code
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
        name: code
messages:
  - message: MT540
    sequences:
      SETDET: {}
    fields:
      - path: SETDET
        tag: 22H
        qualifier: REDE
        name: settlement_method
        type: code
        entity: settlement_indicator
        column: method
"#,
        )
        .expect("schema loads");

        let rendered = render_message(
            &catalog,
            &RenderRequest {
                message_id: "msg-1".to_string(),
                message_type: "MT540".to_string(),
                envelope: render_envelope(),
                rows: vec![RenderRow {
                    table: "settlement_indicator".to_string(),
                    values: BTreeMap::from([
                        ("message_id".to_string(), "msg-1".to_string()),
                        ("sequence_path".to_string(), "SETDET[0]".to_string()),
                        ("method".to_string(), "RECE".to_string()),
                        (
                            "method__render_settlement_method".to_string(),
                            "DELI".to_string(),
                        ),
                    ]),
                }],
            },
        )
        .expect("message renders");

        assert!(rendered.contains(":22H::REDE//RECE\n"));
    }

    #[test]
    fn render_reports_field_cardinality_violations_per_sequence_occurrence() {
        let catalog = SchemaCatalog::from_yaml_str(
            r#"
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
    fields:
      - path: STAT/TRAN
        tag: 20C
        qualifier: RELA
        name: related_reference
        type: reference
        min: 1
        max: 1
        entity: transaction
        column: related_reference
"#,
        )
        .expect("schema loads");

        let rendered = render_message(
            &catalog,
            &RenderRequest {
                message_id: "msg-1".to_string(),
                message_type: "MT537".to_string(),
                envelope: render_envelope(),
                rows: vec![
                    render_row(
                        "transaction",
                        "STAT[0]/TRAN[0]",
                        "related_reference",
                        "REL1",
                    ),
                    render_row(
                        "transaction",
                        "STAT[0]/TRAN[1]",
                        "related_reference",
                        "REL2",
                    ),
                ],
            },
        )
        .expect("one field per repeated sequence occurrence is valid");

        assert!(rendered.contains(":20C::RELA//REL1\n"));
        assert!(rendered.contains(":20C::RELA//REL2\n"));

        let error = render_message(
            &catalog,
            &RenderRequest {
                message_id: "msg-1".to_string(),
                message_type: "MT537".to_string(),
                envelope: render_envelope(),
                rows: vec![
                    render_row(
                        "transaction",
                        "STAT[0]/TRAN[0]",
                        "related_reference",
                        "REL1",
                    ),
                    render_row(
                        "transaction",
                        "STAT[0]/TRAN[0]",
                        "related_reference",
                        "REL1-DUP",
                    ),
                ],
            },
        )
        .expect_err("duplicate field in one sequence occurrence should fail");

        assert_eq!(
            error,
            RenderError::CardinalityViolation {
                field: "related_reference".to_string(),
                table: "transaction".to_string(),
                column: "related_reference".to_string(),
                sequence_path: "STAT[0]/TRAN[0]".to_string(),
                min: Some(1),
                max: Some(1),
                actual: 2,
            }
        );
    }

    #[test]
    fn render_reports_non_repeatable_sequence_cardinality() {
        let catalog = SchemaCatalog::from_yaml_str(
            r#"
field_types:
  - name: text
    pattern:
      - kind: rest
        name: value
messages:
  - message: MT537
    sequences:
      GENL:
        repeat: false
    fields:
      - path: GENL
        tag: 23G
        name: function
        type: text
        entity: statement
        column: function
"#,
        )
        .expect("schema loads");

        let error = render_message(
            &catalog,
            &RenderRequest {
                message_id: "msg-1".to_string(),
                message_type: "MT537".to_string(),
                envelope: render_envelope(),
                rows: vec![
                    render_row("statement", "GENL[0]", "function", "NEWM"),
                    render_row("statement", "GENL[1]", "function", "NEWM"),
                ],
            },
        )
        .expect_err("duplicate non-repeatable sequence should fail");

        assert_eq!(
            error,
            RenderError::SequenceValidation {
                issue: SequenceValidationIssue::Cardinality {
                    sequence_path: "GENL".to_string(),
                    sequence: "GENL".to_string(),
                    parent_path: None,
                    min: None,
                    max: Some(1),
                    actual: 2,
                },
            }
        );
    }

    #[test]
    fn render_reports_missing_required_value() {
        let catalog = SchemaCatalog::from_yaml_str(SCHEMA).expect("schema loads");
        let error = render_message(
            &catalog,
            &RenderRequest {
                message_id: "msg-1".to_string(),
                message_type: "MT540".to_string(),
                envelope: render_envelope(),
                rows: Vec::new(),
            },
        )
        .expect_err("render should fail when required data is absent");

        assert_eq!(
            error,
            RenderError::MissingRequiredValue {
                field: "sender_reference".to_string(),
                table: "settlement_instruction".to_string(),
                column: "sender_reference".to_string(),
            }
        );
    }

    #[test]
    fn reports_schema_validation_errors() {
        let catalog = SchemaCatalog::from_yaml_str(
            r#"
field_types:
  - name: reference
    pattern: []
messages:
  - message: MT999
    sequences: {}
    fields:
      - path: MISSING
        tag: 20C
        name: bad
        type: missing_type
        entity: ""
        column: ""
"#,
        )
        .expect("schema loads");

        let error = catalog.validate().expect_err("schema should be invalid");
        expect![[r#"
            [
                EmptyFieldTypePattern {
                    name: "reference",
                },
                FieldTypeNotFound {
                    message: "MT999",
                    field: "bad",
                    field_type: "missing_type",
                },
                SequenceNotFound {
                    message: "MT999",
                    field: "bad",
                    path: "MISSING",
                },
                EmptyFieldMapping {
                    message: "MT999",
                    field: "bad",
                },
            ]
        "#]]
        .assert_debug_eq(&error.issues);
    }

    #[test]
    fn matches_parsed_fields_to_schema_rules() {
        let catalog = SchemaCatalog::from_yaml_str(SCHEMA).expect("schema loads");
        catalog.validate().expect("schema validates");
        let schema = catalog.message("MT540").expect("MT540 schema");
        let parsed = parse_message(
            br#"{1:F01BANKBEBBAXXX0000000000}{2:I540BANKDEFFXXXXN}{4:
:16R:GENL
:20C::SEME//ABC123
:98A::PREP//20260511
:16R:LINK
:20C::RELA//REL1
:16S:LINK
:16S:GENL
-}"#,
        );

        let message_match = match_message(schema, &parsed);

        assert!(message_match.missing_required.is_empty());
        assert_eq!(message_match.matched_fields.len(), 3);
        assert_eq!(
            message_match.matched_fields[0].rule.name,
            "sender_reference"
        );
        assert_eq!(
            message_match.matched_fields[2].rule.name,
            "related_reference"
        );
    }

    #[test]
    fn qualifier_matching_accepts_pipe_delimited_alternatives() {
        assert!(qualifier_matches(Some("SETT|ESET"), Some(b"SETT")));
        assert!(qualifier_matches(Some("SETT|ESET"), Some(b"ESET")));
        assert!(!qualifier_matches(Some("SETT|ESET"), Some(b"TRAD")));
    }

    #[test]
    fn path_matching_accepts_pipe_delimited_alternatives() {
        let root_fiac = [SequenceFrame {
            name: b"FIAC",
            occurrence: 0,
        }];
        let nested_fiac = [
            SequenceFrame {
                name: b"TRADDET",
                occurrence: 0,
            },
            SequenceFrame {
                name: b"FIAC",
                occurrence: 0,
            },
        ];

        assert!(path_matches("TRADDET/FIAC|FIAC", &root_fiac));
        assert!(path_matches("TRADDET/FIAC|FIAC", &nested_fiac));
        assert!(!path_matches("TRADDET/FIAC|FIAC", &[]));
        assert!(path_matches_schema_path("TRADDET/FIAC|FIAC", "FIAC[0]"));
        assert!(path_matches_schema_path(
            "TRADDET/FIAC|FIAC",
            "TRADDET[0]/FIAC[0]"
        ));
    }

    #[test]
    fn parses_matched_fields_with_reusable_field_types() {
        let catalog = SchemaCatalog::from_yaml_str(SCHEMA).expect("schema loads");
        catalog.validate().expect("schema validates");
        let schema = catalog.message("MT540").expect("MT540 schema");
        let parsed = parse_message(
            br#"{1:F01BANKBEBBAXXX0000000000}{2:I540BANKDEFFXXXXN}{4:
:16R:GENL
:20C::SEME//ABC123
:98A::PREP//20260511
:16R:LINK
:20C::RELA//REL1
:16S:LINK
:16S:GENL
-}"#,
        );

        let message_match = match_and_parse_message(&catalog, schema, &parsed);

        assert!(message_match.parse_errors.is_empty());
        assert!(message_match.missing_required.is_empty());
        assert_eq!(message_match.matched_fields.len(), 3);
        assert_eq!(
            message_match.matched_fields[0].captures,
            vec![
                CapturedFieldValue {
                    name: "qualifier".to_string(),
                    value: &b"SEME"[..],
                },
                CapturedFieldValue {
                    name: "value".to_string(),
                    value: &b"ABC123"[..],
                },
            ]
        );
        assert_eq!(
            message_match.matched_fields[1].captures,
            vec![
                CapturedFieldValue {
                    name: "qualifier".to_string(),
                    value: &b"PREP"[..],
                },
                CapturedFieldValue {
                    name: "date".to_string(),
                    value: &b"20260511"[..],
                },
            ]
        );
    }

    #[test]
    fn matches_generic_option_letter_tags() {
        let catalog = SchemaCatalog::from_yaml_str(
            r#"
field_types:
  - name: date_yyyymmdd
    pattern:
      - kind: literal
        value: ":"
      - kind: capture
        name: qualifier
        value_type: alpha_num
        length: 4
      - kind: literal
        value: "//"
      - kind: capture
        name: date
        value_type: digits
        length: 8
messages:
  - message: MT537
    sequences:
      GENL: {}
    fields:
      - path: GENL
        tag: 98a
        options: [A, C, E]
        name: statement_date
        type: date_yyyymmdd
        entity: statement
        column: statement_date
"#,
        )
        .expect("schema loads");
        catalog.validate().expect("schema validates");
        let schema = catalog.message("MT537").expect("MT537 schema");
        let parsed = parse_message(
            br#"{4:
:16R:GENL
:98A::STAT//20260511
:16S:GENL
-}"#,
        );

        let message_match = match_message(schema, &parsed);

        assert_eq!(message_match.matched_fields.len(), 1);
        assert!(message_match.missing_required.is_empty());
    }

    #[test]
    fn parses_generic_tags_with_option_specific_field_types() {
        let catalog = SchemaCatalog::from_yaml_str(
            r#"
field_types:
  - name: date_yyyymmdd
    pattern:
      - kind: literal
        value: ":"
      - kind: capture
        name: qualifier
        value_type: alpha_num
        length: 4
      - kind: literal
        value: "//"
      - kind: capture
        name: date
        value_type: digits
        length: 8
  - name: datetime_text
    pattern:
      - kind: rest
        name: value
messages:
  - message: MT537
    sequences:
      GENL: {}
    fields:
      - path: GENL
        tag: 98a
        options: [A, C]
        name: statement_datetime
        type: datetime_text
        option_types:
          A: date_yyyymmdd
          C: datetime_text
        entity: statement
        column: statement_datetime
"#,
        )
        .expect("schema loads");
        catalog.validate().expect("schema validates");
        let schema = catalog.message("MT537").expect("MT537 schema");
        let parsed = parse_message(
            br#"{4:
:16R:GENL
:98A::STAT//20260511
:98C::PREP//20260511121030
:16S:GENL
-}"#,
        );

        let message_match = match_and_parse_message(&catalog, schema, &parsed);

        assert!(message_match.parse_errors.is_empty());
        assert_eq!(message_match.matched_fields.len(), 2);
        assert_eq!(
            message_match.matched_fields[0].captures,
            vec![
                CapturedFieldValue {
                    name: "qualifier".to_string(),
                    value: &b"STAT"[..],
                },
                CapturedFieldValue {
                    name: "date".to_string(),
                    value: &b"20260511"[..],
                },
            ]
        );
        assert_eq!(
            message_match.matched_fields[1].captures,
            vec![CapturedFieldValue {
                name: "value".to_string(),
                value: &b":PREP//20260511121030"[..],
            }]
        );
    }

    #[test]
    fn rejects_options_on_concrete_tags() {
        let catalog = SchemaCatalog::from_yaml_str(
            r#"
field_types:
  - name: text
    pattern:
      - kind: rest
        name: value
messages:
  - message: MT537
    sequences:
      GENL: {}
    fields:
      - path: GENL
        tag: 98A
        options: [A, C]
        name: invalid
        type: text
        entity: statement
        column: invalid
"#,
        )
        .expect("schema loads");

        let error = catalog.validate().expect_err("schema should be invalid");
        assert!(error.issues.iter().any(|issue| matches!(
            issue,
            SchemaValidationIssue::InvalidTagOptions { field, tag, .. }
                if field == "invalid" && tag == "98A"
        )));
    }

    #[test]
    fn render_validation_reports_ambiguous_option_and_missing_qualifier() {
        let catalog = SchemaCatalog::from_yaml_str(
            r#"
field_types:
  - name: date_yyyymmdd
    pattern:
      - kind: literal
        value: ":"
      - kind: capture
        name: qualifier
        value_type: alpha_num
        length: 4
      - kind: literal
        value: "//"
      - kind: capture
        name: date
        value_type: digits
        length: 8
messages:
  - message: MT540
    sequences:
      GENL: {}
    fields:
      - path: GENL
        tag: 98a
        options: [A, C]
        name: preparation_date
        type: date_yyyymmdd
        entity: settlement_instruction
        column: preparation_date
"#,
        )
        .expect("schema loads");

        catalog.validate().expect("base schema remains valid");
        let error = catalog
            .validate_rendering()
            .expect_err("render metadata should be incomplete");

        assert!(error.issues.iter().any(|issue| matches!(
            issue,
            SchemaValidationIssue::MissingRenderOption { field, tag, .. }
                if field == "preparation_date" && tag == "98a"
        )));
        assert!(error.issues.iter().any(|issue| matches!(
            issue,
            SchemaValidationIssue::MissingRenderQualifier { field, field_type, .. }
                if field == "preparation_date" && field_type == "date_yyyymmdd"
        )));
    }

    #[test]
    fn render_validation_rejects_invalid_render_option() {
        let catalog = SchemaCatalog::from_yaml_str(
            r#"
field_types:
  - name: text
    pattern:
      - kind: rest
        name: value
messages:
  - message: MT540
    sequences:
      GENL: {}
    fields:
      - path: GENL
        tag: 95a
        options: [P, Q]
        name: place
        type: text
        entity: settlement_party
        column: place
        render:
          option: R
"#,
        )
        .expect("schema loads");

        let error = catalog
            .validate_rendering()
            .expect_err("invalid render option should fail");

        assert!(error.issues.iter().any(|issue| matches!(
            issue,
            SchemaValidationIssue::InvalidRenderOption { field, option, .. }
                if field == "place" && option == "R"
        )));
    }

    #[test]
    fn render_validation_rejects_duplicate_normalized_mappings() {
        let catalog = SchemaCatalog::from_yaml_str(
            r#"
field_types:
  - name: code
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
        name: code
messages:
  - message: MT540
    sequences:
      SETDET: {}
    fields:
      - path: SETDET
        tag: 22F
        qualifier: STCO
        name: settlement_indicator
        type: code
        entity: settlement_indicator
        column: indicator
      - path: SETDET
        tag: 22H
        qualifier: REDE
        name: settlement_method
        type: code
        entity: settlement_indicator
        column: indicator
"#,
        )
        .expect("schema loads");

        catalog.validate().expect("base schema remains valid");
        let error = catalog
            .validate_rendering()
            .expect_err("duplicate render mappings should fail");

        assert_eq!(
            error.issues,
            vec![SchemaValidationIssue::DuplicateRenderMapping {
                message: "MT540".to_string(),
                entity: "settlement_indicator".to_string(),
                path: "SETDET".to_string(),
                column: "indicator".to_string(),
                fields: vec![
                    "settlement_indicator".to_string(),
                    "settlement_method".to_string(),
                ],
            }]
        );
    }

    #[test]
    fn reports_field_cardinality_violations() {
        let catalog = SchemaCatalog::from_yaml_str(
            r#"
field_types:
  - name: text
    pattern:
      - kind: rest
        name: value
messages:
  - message: MT537
    sequences:
      GENL: {}
    fields:
      - path: GENL
        tag: 23G
        name: function
        type: text
        min: 1
        max: 1
        entity: statement
        column: function
"#,
        )
        .expect("schema loads");
        catalog.validate().expect("schema validates");
        let schema = catalog.message("MT537").expect("MT537 schema");
        let parsed = parse_message(
            br#"{4:
:16R:GENL
:23G:NEWM
:23G:NEWM
:16S:GENL
-}"#,
        );

        let message_match = match_message(schema, &parsed);

        assert_eq!(
            message_match.cardinality_violations,
            vec![CardinalityViolation {
                name: "function".to_string(),
                path: "GENL".to_string(),
                sequence_path: Some("GENL[0]".to_string()),
                tag: "23G".to_string(),
                qualifier: None,
                min: Some(1),
                max: Some(1),
                actual: 2,
            }]
        );
    }

    #[test]
    fn reports_missing_required_fields_per_repeated_sequence_occurrence() {
        let catalog = SchemaCatalog::from_yaml_str(
            r#"
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
        required: true
        min: 1
        max: 1
        entity: mt537_transaction
        column: related_reference
"#,
        )
        .expect("schema loads");
        catalog.validate().expect("schema validates");
        let schema = catalog.message("MT537").expect("MT537 schema");
        let parsed = parse_message(
            br#"{4:
:16R:STAT
:16R:TRAN
:16R:TRANSDET
:20C::RELA//REL1
:16S:TRANSDET
:16S:TRAN
:16R:TRAN
:16R:TRANSDET
:16S:TRANSDET
:16S:TRAN
:16S:STAT
-}"#,
        );

        let message_match = match_message(schema, &parsed);

        assert_eq!(
            message_match.missing_required,
            vec![RequiredFieldMiss {
                name: "related_reference".to_string(),
                path: "STAT/TRAN/TRANSDET".to_string(),
                sequence_path: Some("STAT[0]/TRAN[1]/TRANSDET[0]".to_string()),
                tag: "20C".to_string(),
                qualifier: Some("RELA".to_string()),
            }]
        );
        assert_eq!(
            message_match.cardinality_violations,
            vec![CardinalityViolation {
                name: "related_reference".to_string(),
                path: "STAT/TRAN/TRANSDET".to_string(),
                sequence_path: Some("STAT[0]/TRAN[1]/TRANSDET[0]".to_string()),
                tag: "20C".to_string(),
                qualifier: Some("RELA".to_string()),
                min: Some(1),
                max: Some(1),
                actual: 0,
            }]
        );
    }

    #[test]
    fn reports_sequence_parent_violations() {
        let catalog = SchemaCatalog::from_yaml_str(
            r#"
field_types:
  - name: text
    pattern:
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
    fields:
      - path: TRANSDET
        tag: 35B
        name: instrument
        type: text
        entity: transaction
        column: instrument
"#,
        )
        .expect("schema loads");
        catalog.validate().expect("schema validates");
        let schema = catalog.message("MT537").expect("MT537 schema");
        let parsed = parse_message(
            br#"{4:
:16R:STAT
:16R:TRANSDET
:35B:ISIN GB00B03MLX29
:16S:TRANSDET
:16S:STAT
-}"#,
        );

        let message_match = match_message(schema, &parsed);

        assert_eq!(
            message_match.sequence_issues,
            vec![SequenceValidationIssue::InvalidParent {
                sequence_path: "STAT[0]/TRANSDET[0]".to_string(),
                sequence: "TRANSDET".to_string(),
                expected_parents: vec!["TRAN".to_string()],
                actual_parent: Some("STAT".to_string()),
            }]
        );
    }

    #[test]
    fn reports_non_repeatable_sequence_cardinality() {
        let catalog = SchemaCatalog::from_yaml_str(
            r#"
field_types:
  - name: text
    pattern:
      - kind: rest
        name: value
messages:
  - message: MT537
    sequences:
      GENL:
        repeat: false
    fields:
      - path: GENL
        tag: 23G
        name: function
        type: text
        entity: statement
        column: function
"#,
        )
        .expect("schema loads");
        catalog.validate().expect("schema validates");
        let schema = catalog.message("MT537").expect("MT537 schema");
        let parsed = parse_message(
            br#"{4:
:16R:GENL
:23G:NEWM
:16S:GENL
:16R:GENL
:23G:NEWM
:16S:GENL
-}"#,
        );

        let message_match = match_message(schema, &parsed);

        assert_eq!(
            message_match.sequence_issues,
            vec![SequenceValidationIssue::Cardinality {
                sequence_path: "GENL".to_string(),
                sequence: "GENL".to_string(),
                parent_path: None,
                min: None,
                max: Some(1),
                actual: 2,
            }]
        );
    }

    #[test]
    fn reports_field_type_parse_errors() {
        let catalog = SchemaCatalog::from_yaml_str(SCHEMA).expect("schema loads");
        catalog.validate().expect("schema validates");
        let schema = catalog.message("MT540").expect("MT540 schema");
        let parsed = parse_message(
            br#"{1:F01BANKBEBBAXXX0000000000}{2:I540BANKDEFFXXXXN}{4:
:16R:GENL
:20C::SEME//ABC123
:98A::PREP//202605AA
:16S:GENL
-}"#,
        );

        let message_match = match_and_parse_message(&catalog, schema, &parsed);

        assert_eq!(message_match.matched_fields.len(), 1);
        assert_eq!(message_match.parse_errors.len(), 1);
        assert_eq!(
            message_match.parse_errors[0].error,
            FieldParseError::InvalidCaptureValue {
                name: "date".to_string(),
                value_type: CaptureValueType::Digits,
                offset: 7,
            }
        );
    }

    #[test]
    fn infers_database_layout_from_schema_mappings() {
        let catalog = SchemaCatalog::from_yaml_str(SCHEMA).expect("schema loads");
        catalog.validate().expect("schema validates");

        let layout = infer_database_layout(&catalog);

        let table_names: Vec<_> = layout
            .tables
            .iter()
            .map(|table| table.name.as_str())
            .collect();
        assert_eq!(
            table_names,
            vec![
                "swift_raw_messages",
                "swift_fields",
                "swift_parse_errors",
                "message_reference",
                "settlement_instruction",
            ]
        );

        let settlement = layout
            .tables
            .iter()
            .find(|table| table.name == "settlement_instruction")
            .expect("settlement table");
        assert_eq!(
            settlement.columns,
            vec![
                ColumnLayout {
                    name: "message_id".to_string(),
                    logical_type: LogicalColumnType::Text,
                    required: true,
                },
                ColumnLayout {
                    name: "preparation_date".to_string(),
                    logical_type: LogicalColumnType::Date,
                    required: false,
                },
                ColumnLayout {
                    name: "preparation_date__render_preparation_date".to_string(),
                    logical_type: LogicalColumnType::Text,
                    required: false,
                },
                ColumnLayout {
                    name: "sender_reference".to_string(),
                    logical_type: LogicalColumnType::Text,
                    required: true,
                },
                ColumnLayout {
                    name: "sender_reference__render_sender_reference".to_string(),
                    logical_type: LogicalColumnType::Text,
                    required: false,
                },
                ColumnLayout {
                    name: "sequence_path".to_string(),
                    logical_type: LogicalColumnType::Text,
                    required: true,
                },
            ]
        );
    }

    #[test]
    fn infers_datetime_text_columns_as_text() {
        let catalog = SchemaCatalog::from_yaml_str(
            r#"
field_types:
  - name: settlement_datetime_text
    pattern:
      - kind: rest
        name: value
messages:
  - message: MT545
    sequences:
      GENL: {}
    fields:
      - path: GENL
        tag: 98a
        name: preparation_date
        type: settlement_datetime_text
        entity: settlement_instruction
        column: preparation_date
"#,
        )
        .expect("schema loads");
        catalog.validate().expect("schema validates");

        let layout = infer_database_layout(&catalog);
        let settlement = layout
            .tables
            .iter()
            .find(|table| table.name == "settlement_instruction")
            .expect("settlement table");

        let preparation_date = settlement
            .columns
            .iter()
            .find(|column| column.name == "preparation_date")
            .expect("preparation date column");
        assert_eq!(preparation_date.logical_type, LogicalColumnType::Text);
    }

    #[test]
    fn reports_missing_required_fields() {
        let catalog = SchemaCatalog::from_yaml_str(SCHEMA).expect("schema loads");
        catalog.validate().expect("schema validates");
        let schema = catalog.message("MT540").expect("MT540 schema");
        let parsed = parse_message(
            br#"{1:F01BANKBEBBAXXX0000000000}{2:I540BANKDEFFXXXXN}{4:
:16R:GENL
:98A::PREP//20260511
:16S:GENL
-}"#,
        );

        let message_match = match_message(schema, &parsed);

        assert_eq!(
            message_match.missing_required,
            vec![RequiredFieldMiss {
                name: "sender_reference".to_string(),
                path: "GENL".to_string(),
                sequence_path: Some("GENL[0]".to_string()),
                tag: "20C".to_string(),
                qualifier: Some("SEME".to_string()),
            }]
        );
    }

    // ---- Anchored sequences (ADR-0013): MT940-style flat repeating groups ----
    // with NO :16R:/:16S: wrapper in the wire format.

    const MT940_LIKE_SCHEMA: &str = r"
field_types:
  - name: text
    pattern:
      - kind: rest
        name: value
messages:
  - message: MT940LIKE
    sequences:
      ENTRY:
        anchor_tag: '61'
        member_tags: ['86']
        repeat: true
    fields:
      - path: $
        tag: '20'
        name: reference
        type: text
        required: true
        entity: statement
        column: reference
      - path: ENTRY
        tag: '61'
        name: statement_line
        type: text
        required: true
        entity: entry
        column: line
      - path: ENTRY
        tag: '86'
        name: entry_narrative
        type: text
        entity: entry
        column: narrative
      - path: $
        tag: '62F'
        name: closing_balance
        type: text
        required: true
        entity: statement
        column: closing_balance
";

    #[test]
    fn anchored_sequence_scopes_each_entry_independently() {
        let catalog = SchemaCatalog::from_yaml_str(MT940_LIKE_SCHEMA).expect("schema loads");
        catalog.validate().expect("schema validates");
        let schema = catalog.message("MT940LIKE").expect("schema present");
        let anchored = anchored_sequences(schema);
        assert_eq!(anchored.len(), 1, "one anchored sequence declared");

        let raw = b"{4:\n:20:STMT1\n:61:LINE1\n:86:Narrative one\n:61:LINE2\n:86:Narrative two\n:62F:CLOSE\n-}";
        let parsed = swift_core::parse_message_with_sequences(raw, &anchored);
        assert!(parsed.diagnostics.is_empty(), "diagnostics: {:?}", parsed.diagnostics);

        let matched = match_and_parse_message(&catalog, schema, &parsed);
        assert!(matched.missing_required.is_empty(), "{:?}", matched.missing_required);
        assert!(matched.parse_errors.is_empty(), "{:?}", matched.parse_errors);

        // Each entry's :61: and :86: share one scope; the two entries are distinct.
        let scopes: Vec<Option<String>> = matched
            .matched_fields
            .iter()
            .map(|m| sequence_path_to_string(m.field.sequence_path.as_slice()))
            .collect();
        assert_eq!(
            scopes,
            vec![
                None,                        // :20:
                Some("ENTRY[0]".to_string()), // :61: line 1
                Some("ENTRY[0]".to_string()), // :86: narrative 1 — same scope
                Some("ENTRY[1]".to_string()), // :61: line 2
                Some("ENTRY[1]".to_string()), // :86: narrative 2 — same scope
                None,                        // :62F:
            ]
        );
    }

    #[test]
    fn render_does_not_emit_16r_16s_for_anchored_sequences() {
        let catalog = SchemaCatalog::from_yaml_str(MT940_LIKE_SCHEMA).expect("schema loads");
        catalog.validate().expect("schema validates");

        let rendered = render_message(
            &catalog,
            &RenderRequest {
                message_id: "msg-1".to_string(),
                message_type: "MT940LIKE".to_string(),
                envelope: RenderEnvelope {
                    block1: "F01BANKGB22AXXX0000000000".to_string(),
                    block2: "I940BANKDEFFXXXXN".to_string(),
                    block3: None,
                    block5: None,
                },
                rows: vec![
                    render_row("statement", "$", "reference", "STMT1"),
                    render_row("entry", "ENTRY[0]", "line", "LINE1"),
                    render_row("entry", "ENTRY[0]", "narrative", "Narrative one"),
                    render_row("entry", "ENTRY[1]", "line", "LINE2"),
                    render_row("entry", "ENTRY[1]", "narrative", "Narrative two"),
                    render_row("statement", "$", "closing_balance", "CLOSE"),
                ],
            },
        )
        .expect("renders");

        assert!(
            !rendered.contains("16R") && !rendered.contains("16S"),
            "anchored sequences must not render :16R:/:16S: wrapper lines:\n{rendered}"
        );
        // Fields appear in wire order, byte-exact, with no extra markers.
        let block4_line_count = rendered.lines().filter(|l| l.starts_with(':')).count();
        assert_eq!(block4_line_count, 6, "exactly the 6 data lines, no wrapper lines:\n{rendered}");
        assert!(rendered.contains(":61:LINE1\n"));
        assert!(rendered.contains(":86:Narrative one\n"));
        assert!(rendered.contains(":61:LINE2\n"));
        assert!(rendered.contains(":86:Narrative two\n"));
    }
}
