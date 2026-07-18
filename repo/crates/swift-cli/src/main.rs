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

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use duckdb::Connection;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::fs;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use swift_core::parse_message;
use swift_db::{
    materialize_message, materialize_structural_message, InboundSource, MigrationSink,
    ParsedOutputBatch, ParsedSink,
};
use swift_duckdb::{DuckDbExportFormat, DuckDbExportOptions, DuckDbInboundConfig, DuckDbStore};
use swift_schema::{
    infer_database_layout, match_and_parse_message, render_message, FieldPatternStep,
    RenderEnvelope, RenderRequest, SchemaCatalog,
};

#[derive(Debug, Parser)]
#[command(name = "swiftpipe")]
#[command(about = "Schema-driven SWIFT FIN ingestion")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Schema {
        #[command(subcommand)]
        command: SchemaCommand,
    },
    Migrate {
        #[arg(short, long, default_value = "swiftpipe.toml")]
        config: PathBuf,
    },
    Run {
        #[arg(short, long, default_value = "swiftpipe.toml")]
        config: PathBuf,
        #[arg(long)]
        limit: Option<usize>,
    },
    Export {
        #[arg(short, long, default_value = "swiftpipe.toml")]
        config: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        #[arg(long = "format", value_enum, default_values_t = [CliExportFormat::Csv])]
        formats: Vec<CliExportFormat>,
        #[arg(long)]
        parquet_row_group_size: Option<NonZeroUsize>,
    },
    Render {
        #[arg(short, long, default_value = "swiftpipe.toml")]
        config: PathBuf,
        #[arg(long)]
        message_id: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        message_type: Option<String>,
        #[arg(long)]
        validate: bool,
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(long)]
        output_dir: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
enum SchemaCommand {
    Validate {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
    RenderValidate {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
    Coverage {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
}

#[derive(Debug, Clone, Deserialize)]
struct SwiftPipeConfig {
    source: SourceConfig,
    sink: SinkConfig,
    schemas: SchemasConfig,
    #[serde(default)]
    runtime: RuntimeConfig,
    #[serde(default)]
    outbound: OutboundConfig,
}

#[derive(Debug, Clone, Deserialize)]
struct SourceConfig {
    #[serde(default = "duckdb_adapter")]
    adapter: String,
    database: PathBuf,
    #[serde(default = "default_inbound_table")]
    table: String,
    #[serde(default = "default_id_column")]
    id_column: String,
    #[serde(default = "default_message_type_column")]
    message_type_column: String,
    #[serde(default = "default_body_column")]
    body_column: String,
    #[serde(default = "default_processed_column")]
    processed_column: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct SinkConfig {
    #[serde(default = "duckdb_adapter")]
    adapter: String,
    database: PathBuf,
    #[serde(default = "default_write_batch_size")]
    write_batch_size: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct SchemasConfig {
    paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
struct RuntimeConfig {
    #[serde(default = "default_batch_size")]
    batch_size: usize,
    #[serde(default)]
    fail_on_missing_schema: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct OutboundConfig {
    #[serde(default = "default_block1_template")]
    block1: String,
    #[serde(default = "default_block2_template")]
    block2: String,
    #[serde(default)]
    block3: Option<String>,
    #[serde(default)]
    block5: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CliExportFormat {
    Csv,
    Parquet,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            batch_size: default_batch_size(),
            fail_on_missing_schema: false,
        }
    }
}

impl Default for OutboundConfig {
    fn default() -> Self {
        Self {
            block1: default_block1_template(),
            block2: default_block2_template(),
            block3: None,
            block5: None,
        }
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "CLI dispatch is kept in one visible command table"
)]
fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Schema { command } => match command {
            SchemaCommand::Validate { paths } => {
                let catalog = load_catalog(&paths)?;
                catalog.validate()?;
                println!(
                    "schema valid: {} field type(s), {} message schema(s)",
                    catalog.field_types.len(),
                    catalog.messages.len()
                );
            }
            SchemaCommand::RenderValidate { paths } => {
                let catalog = load_catalog(&paths)?;
                catalog.validate_rendering()?;
                println!(
                    "schema render-valid: {} field type(s), {} message schema(s)",
                    catalog.field_types.len(),
                    catalog.messages.len()
                );
            }
            SchemaCommand::Coverage { paths } => {
                let catalog = load_catalog(&paths)?;
                catalog.validate()?;
                print_schema_coverage(&catalog);
            }
        },
        Command::Migrate { config } => {
            let config = load_config(&config)?;
            ensure_duckdb(&config.source.adapter, "source")?;
            ensure_duckdb(&config.sink.adapter, "sink")?;
            let catalog = load_catalog(&config.schemas.paths)?;
            catalog.validate()?;
            let layout = infer_database_layout(&catalog);
            let conn = Connection::open(&config.sink.database).with_context(|| {
                format!(
                    "failed to open DuckDB sink {}",
                    config.sink.database.display()
                )
            })?;
            let mut store = duckdb_store(conn, &config);
            store.apply_layout(&layout)?;
            println!("migrated {} table(s)", layout.tables.len());
        }
        Command::Run { config, limit } => {
            let config = load_config(&config)?;
            ensure_duckdb(&config.source.adapter, "source")?;
            ensure_duckdb(&config.sink.adapter, "sink")?;
            if config.source.database != config.sink.database {
                bail!("separate DuckDB source and sink files are not supported by this first CLI slice");
            }

            let catalog = load_catalog(&config.schemas.paths)?;
            catalog.validate()?;
            let layout = infer_database_layout(&catalog);
            let conn = Connection::open(&config.sink.database).with_context(|| {
                format!("failed to open DuckDB {}", config.sink.database.display())
            })?;
            let mut store = duckdb_store(conn, &config);
            store.apply_layout(&layout)?;

            let batch_limit = limit.unwrap_or(config.runtime.batch_size);
            let inbound = store.read_batch(batch_limit)?;
            let mut output = ParsedOutputBatch::empty();
            let mut processed_ids = Vec::new();

            for message in &inbound.messages {
                let parsed = parse_message(message.body.as_bytes());
                let message_output = match materialize_message(&catalog, message, &parsed) {
                    Ok(output) => output,
                    Err(error) if !config.runtime.fail_on_missing_schema => {
                        let mut fallback = materialize_structural_message(message, &parsed);
                        fallback.parse_errors.push(swift_db::ParseErrorRow {
                            message_id: message.id.clone(),
                            error: error.to_string(),
                        });
                        fallback
                    }
                    Err(error) => return Err(error.into()),
                };
                output.extend(message_output);
                processed_ids.push(message.id.clone());
            }

            store.write_batch(&output)?;
            store.mark_processed(&processed_ids)?;
            println!(
                "processed {} message(s), wrote {} field row(s), {} normalized row(s), {} parse error row(s)",
                processed_ids.len(),
                output.fields.len(),
                output.normalized_rows.len(),
                output.parse_errors.len()
            );
        }
        Command::Export {
            config,
            output,
            formats,
            parquet_row_group_size,
        } => {
            let config = load_config(&config)?;
            ensure_duckdb(&config.sink.adapter, "sink")?;
            let catalog = load_catalog(&config.schemas.paths)?;
            catalog.validate()?;
            let layout = infer_database_layout(&catalog);
            let conn = Connection::open(&config.sink.database).with_context(|| {
                format!("failed to open DuckDB {}", config.sink.database.display())
            })?;
            let store = duckdb_store(conn, &config);
            let formats = formats
                .into_iter()
                .map(DuckDbExportFormat::from)
                .collect::<Vec<_>>();
            let exported = store.export_layout_with_options(
                &layout,
                &output,
                &formats,
                DuckDbExportOptions {
                    parquet_row_group_size: parquet_row_group_size.map(NonZeroUsize::get),
                },
            )?;
            println!(
                "exported {} file(s) to {}",
                exported.len(),
                output.display()
            );
        }
        Command::Render {
            config,
            message_id,
            all,
            message_type,
            validate,
            output,
            output_dir,
        } => {
            let config = load_config(&config)?;
            ensure_duckdb(&config.sink.adapter, "sink")?;
            let catalog = load_catalog(&config.schemas.paths)?;
            catalog.validate_rendering()?;
            let layout = infer_database_layout(&catalog);
            let conn = Connection::open(&config.sink.database).with_context(|| {
                format!("failed to open DuckDB {}", config.sink.database.display())
            })?;
            let mut store = duckdb_store(conn, &config);
            store.apply_layout(&layout)?;

            if all {
                if message_id.is_some() || message_type.is_some() || output.is_some() {
                    bail!(
                        "--all cannot be combined with --message-id, --message-type, or --output"
                    );
                }
                let output_dir = output_dir.context("--output-dir is required with --all")?;
                fs::create_dir_all(&output_dir)
                    .with_context(|| format!("failed to create {}", output_dir.display()))?;
                let headers = store
                    .read_render_message_headers()
                    .context("failed to list renderable messages")?;
                let mut rendered_messages = Vec::new();
                let mut output_paths = BTreeSet::new();
                for (message_id, message_type) in &headers {
                    let output = output_dir.join(format!("{}.fin", output_stem(message_id)));
                    if !output_paths.insert(output.clone()) {
                        bail!(
                            "multiple message ids map to the same output file: {}",
                            output.display()
                        );
                    }
                    let rendered = render_one_message(
                        &catalog,
                        &layout,
                        &store,
                        &config.outbound,
                        message_id,
                        message_type,
                        validate,
                    )
                    .with_context(|| format!("failed to render {message_id} ({message_type})"))?;
                    rendered_messages.push((output, rendered));
                }
                for (output, rendered) in rendered_messages {
                    write_file_atomic(&output, &rendered)
                        .with_context(|| format!("failed to write {}", output.display()))?;
                }
                println!(
                    "rendered {} message(s) to {}",
                    headers.len(),
                    output_dir.display()
                );
            } else {
                if output_dir.is_some() {
                    bail!("--output-dir requires --all");
                }
                let message_id =
                    message_id.context("--message-id is required unless --all is set")?;
                let message_type = match message_type {
                    Some(message_type) => message_type,
                    None => store
                        .read_message_type(&message_id)
                        .with_context(|| format!("failed to find message type for {message_id}"))?,
                };
                let rendered = render_one_message(
                    &catalog,
                    &layout,
                    &store,
                    &config.outbound,
                    &message_id,
                    &message_type,
                    validate,
                )
                .with_context(|| format!("failed to render {message_id} ({message_type})"))?;
                if let Some(output) = output {
                    write_file_atomic(&output, &rendered)
                        .with_context(|| format!("failed to write {}", output.display()))?;
                    println!("rendered {message_id} to {}", output.display());
                } else {
                    print!("{rendered}");
                }
            }
        }
    }

    Ok(())
}

fn render_one_message(
    catalog: &SchemaCatalog,
    layout: &swift_schema::DatabaseLayout,
    store: &DuckDbStore,
    outbound: &OutboundConfig,
    message_id: &str,
    message_type: &str,
    validate: bool,
) -> Result<String> {
    let rows = store
        .read_render_rows(layout, message_id)
        .with_context(|| format!("failed to read render rows for {message_id}"))?;
    let rendered = render_message(
        catalog,
        &RenderRequest {
            message_id: message_id.to_string(),
            message_type: message_type.to_string(),
            envelope: render_envelope(outbound, message_id, message_type)?,
            rows,
        },
    )?;
    if validate {
        validate_rendered_message(catalog, message_type, &rendered)?;
    }
    Ok(rendered)
}

#[allow(
    clippy::too_many_lines,
    reason = "coverage output is easier to audit as one report formatter"
)]
fn print_schema_coverage(catalog: &SchemaCatalog) {
    println!("field_types: {}", catalog.field_types.len());
    println!("messages: {}", catalog.messages.len());

    for message in &catalog.messages {
        let option_rules = message
            .fields
            .iter()
            .filter(|field| !field.options.is_empty())
            .count();
        let option_type_rules = message
            .fields
            .iter()
            .filter(|field| !field.option_types.is_empty())
            .count();
        let render_metadata_rules = message
            .fields
            .iter()
            .filter(|field| field.render.is_some())
            .count();
        let render_option_rules = message
            .fields
            .iter()
            .filter(|field| {
                field
                    .render
                    .as_ref()
                    .and_then(|render| render.option.as_ref())
                    .is_some()
            })
            .count();
        let render_qualifier_rules = message
            .fields
            .iter()
            .filter(|field| {
                field.qualifier.is_some()
                    || field
                        .render
                        .as_ref()
                        .and_then(|render| render.qualifier.as_ref())
                        .is_some()
            })
            .count();
        let ambiguous_render_options = message
            .fields
            .iter()
            .filter(|field| {
                field.options.len() > 1
                    && field
                        .render
                        .as_ref()
                        .and_then(|render| render.option.as_ref())
                        .is_none()
            })
            .count();
        let missing_render_qualifiers = message
            .fields
            .iter()
            .filter(|field| {
                field.qualifier.is_none()
                    && field
                        .render
                        .as_ref()
                        .and_then(|render| render.qualifier.as_ref())
                        .is_none()
                    && catalog
                        .field_type(&field.field_type)
                        .is_some_and(|field_type| field_type_uses_capture(field_type, "qualifier"))
            })
            .count();
        let required_fields = message
            .fields
            .iter()
            .filter(|field| field.required || field.min.unwrap_or(0) > 0)
            .count();
        let cardinality_fields = message
            .fields
            .iter()
            .filter(|field| field.min.is_some() || field.max.is_some())
            .count();
        let parented_sequences = message
            .sequences
            .values()
            .filter(|sequence| sequence.parent.is_some() || !sequence.parents.is_empty())
            .count();
        let cardinality_sequences = message
            .sequences
            .values()
            .filter(|sequence| sequence.min.is_some() || sequence.max.is_some() || !sequence.repeat)
            .count();
        let entities: BTreeSet<_> = message
            .fields
            .iter()
            .map(|field| field.entity.as_str())
            .collect();

        println!();
        println!("message: {}", message.message);
        if let Some(coverage) = &message.coverage {
            if let Some(source) = &coverage.source {
                println!("  coverage_source: {source}");
            }
            println!("  exact: {}", coverage.exact);
            if let Some(expected) = coverage.expected_sequences {
                println!(
                    "  sequence_progress: {}/{} ({:.1}%)",
                    message.sequences.len(),
                    expected,
                    percent(message.sequences.len(), expected)
                );
            }
            if let Some(expected) = coverage.expected_fields {
                println!(
                    "  field_progress: {}/{} ({:.1}%)",
                    message.fields.len(),
                    expected,
                    percent(message.fields.len(), expected)
                );
            }
            for note in &coverage.notes {
                println!("  note: {note}");
            }
        }
        println!("  sequences: {}", message.sequences.len());
        println!("  parented_sequences: {parented_sequences}");
        println!("  sequence_cardinality_rules: {cardinality_sequences}");
        println!("  fields: {}", message.fields.len());
        println!("  required_or_min_fields: {required_fields}");
        println!("  field_cardinality_rules: {cardinality_fields}");
        println!("  option_tag_rules: {option_rules}");
        println!("  option_specific_parser_rules: {option_type_rules}");
        println!("  render_metadata_rules: {render_metadata_rules}");
        println!("  render_option_rules: {render_option_rules}");
        println!("  render_qualifier_rules: {render_qualifier_rules}");
        println!("  ambiguous_render_options: {ambiguous_render_options}");
        println!("  missing_render_qualifiers: {missing_render_qualifiers}");
        println!("  normalized_entities: {}", entities.len());
        println!(
            "  entities: {}",
            entities.into_iter().collect::<Vec<_>>().join(", ")
        );
    }
}

fn field_type_uses_capture(field_type: &swift_schema::FieldTypeSchema, capture_name: &str) -> bool {
    field_type.pattern.iter().any(|step| match step {
        FieldPatternStep::Capture { name, .. } | FieldPatternStep::Rest { name } => {
            name == capture_name
        }
        FieldPatternStep::Literal { .. } => false,
    })
}

#[allow(
    clippy::cast_precision_loss,
    reason = "coverage percentages are display-only"
)]
fn percent(actual: usize, expected: usize) -> f64 {
    if expected == 0 {
        100.0
    } else {
        (actual as f64 / expected as f64) * 100.0
    }
}

fn default_write_batch_size() -> usize {
    swift_duckdb::DEFAULT_WRITE_BATCH_SIZE
}

fn duckdb_store(conn: Connection, config: &SwiftPipeConfig) -> DuckDbStore {
    DuckDbStore::new(conn, inbound_config(&config.source))
        .with_write_batch_size(config.sink.write_batch_size)
}

fn load_config(path: &Path) -> Result<SwiftPipeConfig> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read config {}", path.display()))?;
    toml::from_str(&content).with_context(|| format!("failed to parse config {}", path.display()))
}

fn load_catalog(paths: &[PathBuf]) -> Result<SchemaCatalog> {
    let mut catalog = SchemaCatalog {
        field_types: Vec::new(),
        messages: Vec::new(),
    };

    for path in schema_files(paths)? {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read schema {}", path.display()))?;
        let mut partial = SchemaCatalog::from_yaml_str(&content)
            .with_context(|| format!("failed to parse schema {}", path.display()))?;
        for field_type in partial.field_types.drain(..) {
            if let Some(existing) = catalog
                .field_types
                .iter()
                .find(|existing| existing.name == field_type.name)
            {
                if existing != &field_type {
                    bail!(
                        "conflicting duplicate field type '{}' in {}",
                        field_type.name,
                        path.display()
                    );
                }
                continue;
            }
            catalog.field_types.push(field_type);
        }
        catalog.messages.append(&mut partial.messages);
    }

    Ok(catalog)
}

#[cfg(test)]
#[allow(
    clippy::items_after_test_module,
    clippy::needless_raw_string_hashes,
    reason = "schema fixtures are kept visually stable"
)]
mod tests {
    use super::*;

    fn write(path: &Path, contents: &str) {
        fs::write(path, contents).expect("temporary schema file should be written");
    }

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ))
    }

    fn write_yaml_schema(path: &Path) {
        write(
            path,
            r#"field_types:
  - name: duplicate
    pattern:
      - kind: rest
        name: value
messages:
  - message: MT900
"#,
        );
    }

    fn write_conflicting_schema(path: &Path) {
        write(
            path,
            r#"field_types:
  - name: duplicate
    pattern:
      - kind: literal
        value: ":"
messages:
  - message: MT901
"#,
        );
    }

    fn write_identical_schema(path: &Path) {
        write_yaml_schema(path);
    }

    #[test]
    fn load_catalog_allows_identical_duplicate_field_type_across_files() {
        let root = temp_dir("swiftpipe-load-catalog-identical");
        fs::create_dir_all(&root).expect("temp dir should be created");

        let first = root.join("first.yaml");
        let second = root.join("second.yaml");
        write_yaml_schema(&first);
        write_identical_schema(&second);

        let catalog = load_catalog(&[first.clone(), second.clone()])
            .expect("catalog should merge identical field types");
        assert_eq!(catalog.field_types.len(), 1);

        fs::remove_dir_all(&root).expect("temp dir should be removed");
    }

    #[test]
    fn load_catalog_rejects_conflicting_duplicate_field_type_across_files() {
        let root = temp_dir("swiftpipe-load-catalog-conflict");
        fs::create_dir_all(&root).expect("temp dir should be created");

        let first = root.join("first.yaml");
        let second = root.join("second.yaml");
        write_yaml_schema(&first);
        write_conflicting_schema(&second);

        let result = load_catalog(&[first.clone(), second.clone()]);
        assert!(result.is_err());

        fs::remove_dir_all(&root).expect("temp dir should be removed");
    }

    #[test]
    fn write_file_atomic_replaces_existing_file() {
        let root = temp_dir("swiftpipe-atomic-write-replace");
        fs::create_dir_all(&root).expect("temp dir should be created");

        let output = root.join("message.fin");
        fs::write(&output, "old").expect("existing output should be written");

        write_file_atomic(&output, "new").expect("atomic write should succeed");

        assert_eq!(
            fs::read_to_string(&output).expect("output should be readable"),
            "new"
        );
        assert!(
            fs::read_dir(&root)
                .expect("temp dir should be readable")
                .all(|entry| !entry
                    .expect("entry should be readable")
                    .file_name()
                    .to_string_lossy()
                    .contains("swiftpipe-tmp")),
            "temporary output files should be cleaned up"
        );

        fs::remove_dir_all(&root).expect("temp dir should be removed");
    }

    #[test]
    fn write_file_atomic_cleans_temp_file_when_rename_fails() {
        let root = temp_dir("swiftpipe-atomic-write-rename-failure");
        fs::create_dir_all(&root).expect("temp dir should be created");

        let output = root.join("message.fin");
        fs::create_dir(&output).expect("output directory should be created");

        let result = write_file_atomic(&output, "new");

        assert!(result.is_err());
        assert!(output.is_dir(), "existing output directory should remain");
        assert!(
            fs::read_dir(&root)
                .expect("temp dir should be readable")
                .all(|entry| !entry
                    .expect("entry should be readable")
                    .file_name()
                    .to_string_lossy()
                    .contains("swiftpipe-tmp")),
            "temporary output file should be removed after rename failure"
        );

        fs::remove_dir_all(&root).expect("temp dir should be removed");
    }

    #[test]
    fn render_envelope_expands_templates_and_accepts_optional_blocks() {
        let outbound = OutboundConfig {
            block1: "F01{message_id}".to_string(),
            block2: "I{mt}BANKDEFFXXXXN".to_string(),
            block3: Some("{108:{message_id}}".to_string()),
            block5: Some("{CHK:{message_type}}".to_string()),
        };

        let envelope =
            render_envelope(&outbound, "msg-1", "MT540").expect("envelope should render");

        assert_eq!(envelope.block1, "F01msg-1");
        assert_eq!(envelope.block2, "I540BANKDEFFXXXXN");
        assert_eq!(envelope.block3.as_deref(), Some("{108:msg-1}"));
        assert_eq!(envelope.block5.as_deref(), Some("{CHK:MT540}"));
    }

    #[test]
    fn render_envelope_rejects_empty_required_blocks() {
        let outbound = OutboundConfig {
            block1: " ".to_string(),
            ..OutboundConfig::default()
        };

        let error = render_envelope(&outbound, "msg-1", "MT540")
            .expect_err("empty block1 should fail")
            .to_string();

        assert!(error.contains("outbound.block1 must not be empty"));
    }

    #[test]
    fn render_envelope_rejects_empty_optional_blocks() {
        let outbound = OutboundConfig {
            block3: Some(" ".to_string()),
            ..OutboundConfig::default()
        };

        let error = render_envelope(&outbound, "msg-1", "MT540")
            .expect_err("empty block3 should fail")
            .to_string();

        assert!(error.contains("outbound.block3 must be omitted or non-empty"));
    }

    #[test]
    fn render_envelope_rejects_newlines() {
        let outbound = OutboundConfig {
            block2: "I540\nBANKDEFFXXXXN".to_string(),
            ..OutboundConfig::default()
        };

        let error = render_envelope(&outbound, "msg-1", "MT540")
            .expect_err("newline block2 should fail")
            .to_string();

        assert!(error.contains("outbound.block2 must not contain newlines"));
    }
}

fn schema_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for path in paths {
        if path.is_dir() {
            for entry in fs::read_dir(path)
                .with_context(|| format!("failed to read schema directory {}", path.display()))?
            {
                let entry = entry?;
                let entry_path = entry.path();
                if is_yaml_file(&entry_path) {
                    files.push(entry_path);
                }
            }
        } else if path.is_file() {
            files.push(path.clone());
        } else {
            bail!("schema path does not exist: {}", path.display());
        }
    }
    files.sort();
    Ok(files)
}

fn is_yaml_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("yaml" | "yml")
    )
}

fn inbound_config(source: &SourceConfig) -> DuckDbInboundConfig {
    DuckDbInboundConfig {
        inbound_table: source.table.clone(),
        id_column: source.id_column.clone(),
        message_type_column: source.message_type_column.clone(),
        body_column: source.body_column.clone(),
        processed_column: source.processed_column.clone(),
    }
}

fn ensure_duckdb(adapter: &str, label: &str) -> Result<()> {
    if adapter == "duckdb" {
        Ok(())
    } else {
        bail!("unsupported {label} adapter: {adapter}")
    }
}

fn duckdb_adapter() -> String {
    "duckdb".to_string()
}

fn default_inbound_table() -> String {
    "inbound_messages".to_string()
}

fn default_id_column() -> String {
    "id".to_string()
}

fn default_message_type_column() -> String {
    "message_type".to_string()
}

fn default_body_column() -> String {
    "body".to_string()
}

#[allow(
    clippy::unnecessary_wraps,
    reason = "serde default for Option<String> requires an Option-returning helper"
)]
fn default_processed_column() -> Option<String> {
    Some("processed".to_string())
}

fn default_batch_size() -> usize {
    10_000
}

fn default_block1_template() -> String {
    "F01BANKBEBBAXXX0000000000".to_string()
}

fn default_block2_template() -> String {
    "I{mt}BANKDEFFXXXXN".to_string()
}

fn render_template(template: &str, message_id: &str, message_type: &str) -> String {
    template
        .replace("{message_id}", message_id)
        .replace("{message_type}", message_type)
        .replace("{mt}", message_type.trim_start_matches("MT"))
}

fn render_envelope(
    outbound: &OutboundConfig,
    message_id: &str,
    message_type: &str,
) -> Result<RenderEnvelope> {
    let envelope = RenderEnvelope {
        block1: render_template(&outbound.block1, message_id, message_type),
        block2: render_template(&outbound.block2, message_id, message_type),
        block3: outbound
            .block3
            .as_ref()
            .map(|template| render_template(template, message_id, message_type)),
        block5: outbound
            .block5
            .as_ref()
            .map(|template| render_template(template, message_id, message_type)),
    };
    validate_render_envelope(&envelope)?;
    Ok(envelope)
}

fn validate_render_envelope(envelope: &RenderEnvelope) -> Result<()> {
    validate_required_envelope_block("outbound.block1", &envelope.block1)?;
    validate_required_envelope_block("outbound.block2", &envelope.block2)?;
    validate_optional_envelope_block("outbound.block3", envelope.block3.as_deref())?;
    validate_optional_envelope_block("outbound.block5", envelope.block5.as_deref())?;
    Ok(())
}

fn validate_required_envelope_block(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{label} must not be empty");
    }
    validate_envelope_block_text(label, value)
}

fn validate_optional_envelope_block(label: &str, value: Option<&str>) -> Result<()> {
    let Some(value) = value else {
        return Ok(());
    };
    if value.trim().is_empty() {
        bail!("{label} must be omitted or non-empty");
    }
    validate_envelope_block_text(label, value)
}

fn validate_envelope_block_text(label: &str, value: &str) -> Result<()> {
    if value.contains('\n') || value.contains('\r') {
        bail!("{label} must not contain newlines");
    }
    Ok(())
}

fn output_stem(message_id: &str) -> String {
    message_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn write_file_atomic(path: &Path, contents: &str) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let filename = path
        .file_name()
        .and_then(|filename| filename.to_str())
        .context("output path must include a valid file name")?;
    let temp_path = parent.join(format!(
        ".{filename}.swiftpipe-tmp-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .context("system clock is before UNIX epoch")?
            .as_nanos()
    ));

    fs::write(&temp_path, contents)
        .with_context(|| format!("failed to write temporary output {}", temp_path.display()))?;
    if let Err(error) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(error)
            .with_context(|| format!("failed to move temporary output to {}", path.display()));
    }

    Ok(())
}

fn validate_rendered_message(
    catalog: &SchemaCatalog,
    message_type: &str,
    rendered: &str,
) -> Result<()> {
    let parsed = parse_message(rendered.as_bytes());
    if !parsed.diagnostics.is_empty() {
        bail!(
            "rendered message has parse diagnostics: {:?}",
            parsed.diagnostics
        );
    }

    let schema = catalog
        .message(message_type)
        .with_context(|| format!("message schema not found for {message_type}"))?;
    let matched = match_and_parse_message(catalog, schema, &parsed);
    if !matched.parse_errors.is_empty() {
        bail!(
            "rendered message has schema parse errors: {:?}",
            matched.parse_errors
        );
    }
    if !matched.missing_required.is_empty() {
        bail!(
            "rendered message is missing required fields: {:?}",
            matched.missing_required
        );
    }
    if !matched.cardinality_violations.is_empty() {
        bail!(
            "rendered message has cardinality violations: {:?}",
            matched.cardinality_violations
        );
    }
    if !matched.sequence_issues.is_empty() {
        bail!(
            "rendered message has sequence validation issues: {:?}",
            matched.sequence_issues
        );
    }

    Ok(())
}

impl From<CliExportFormat> for DuckDbExportFormat {
    fn from(value: CliExportFormat) -> Self {
        match value {
            CliExportFormat::Csv => Self::Csv,
            CliExportFormat::Parquet => Self::Parquet,
        }
    }
}
