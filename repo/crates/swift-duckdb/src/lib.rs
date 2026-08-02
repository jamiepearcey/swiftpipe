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

//! `DuckDB` adapter for `SwiftPipe` ingestion.

use duckdb::{params_from_iter, Connection, ToSql};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;
use swift_db::{
    FieldRow, InboundMessage, InboundSource, MessageBatch, MigrationSink, NormalizedRow,
    ParseErrorRow, ParsedOutputBatch, ParsedSink, RawMessageRow,
};
use swift_schema::{ColumnLayout, DatabaseLayout, LogicalColumnType, RenderRow, TableLayout};

pub const DEFAULT_WRITE_BATCH_SIZE: usize = 1_000;

pub struct DuckDbStore {
    conn: Connection,
    inbound_table: String,
    id_column: String,
    message_type_column: String,
    body_column: String,
    processed_column: Option<String>,
    write_batch_size: usize,
}

impl fmt::Debug for DuckDbStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DuckDbStore")
            .field("inbound_table", &self.inbound_table)
            .field("id_column", &self.id_column)
            .field("message_type_column", &self.message_type_column)
            .field("body_column", &self.body_column)
            .field("processed_column", &self.processed_column)
            .field("write_batch_size", &self.write_batch_size)
            .finish_non_exhaustive()
    }
}

impl DuckDbStore {
    pub fn new(conn: Connection, config: DuckDbInboundConfig) -> Self {
        Self {
            conn,
            inbound_table: config.inbound_table,
            id_column: config.id_column,
            message_type_column: config.message_type_column,
            body_column: config.body_column,
            processed_column: config.processed_column,
            write_batch_size: DEFAULT_WRITE_BATCH_SIZE,
        }
    }

    #[must_use]
    pub fn with_write_batch_size(mut self, write_batch_size: usize) -> Self {
        self.write_batch_size = write_batch_size.max(1);
        self
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub fn export_layout(
        &self,
        layout: &DatabaseLayout,
        directory: &Path,
        formats: &[DuckDbExportFormat],
    ) -> Result<Vec<ExportedTable>, DuckDbAdapterError> {
        self.export_layout_with_options(layout, directory, formats, DuckDbExportOptions::default())
    }

    pub fn export_layout_with_options(
        &self,
        layout: &DatabaseLayout,
        directory: &Path,
        formats: &[DuckDbExportFormat],
        options: DuckDbExportOptions,
    ) -> Result<Vec<ExportedTable>, DuckDbAdapterError> {
        std::fs::create_dir_all(directory).map_err(DuckDbAdapterError::Io)?;
        let mut exported = Vec::new();

        for table in &layout.tables {
            for format in formats {
                let path = directory.join(format!("{}.{}", table.name, format.extension()));
                let sql = format!(
                    "COPY {} TO '{}' ({})",
                    ident(&table.name),
                    sql_string(path.to_string_lossy().as_ref()),
                    copy_options_sql(*format, options)
                );
                self.conn.execute(&sql, [])?;
                exported.push(ExportedTable {
                    table: table.name.clone(),
                    path,
                    format: *format,
                });
            }
        }

        Ok(exported)
    }

    pub fn read_message_type(&self, message_id: &str) -> Result<String, DuckDbAdapterError> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT message_type FROM swift_raw_messages WHERE message_id = ? ORDER BY message_type",
        )?;
        let result = stmt.query_map([message_id], |row| row.get::<_, String>(0))?;

        let mut message_types: Vec<String> = Vec::new();
        for message_type in result {
            message_types.push(message_type?);
        }

        match message_types.as_slice() {
            [] => Err(DuckDbAdapterError::MessageTypeNotFound {
                message_id: message_id.to_string(),
            }),
            [message_type] => Ok(message_type.clone()),
            _ => Err(DuckDbAdapterError::AmbiguousMessageType {
                message_id: message_id.to_string(),
                message_types,
            }),
        }
    }

    pub fn read_render_message_headers(&self) -> Result<Vec<(String, String)>, DuckDbAdapterError> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT message_id, message_type FROM swift_raw_messages ORDER BY message_id, message_type",
        )?;
        let result = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut headers = Vec::new();
        for header in result {
            headers.push(header?);
        }

        let mut types_by_id = BTreeMap::<String, Vec<String>>::new();
        for (message_id, message_type) in &headers {
            types_by_id
                .entry(message_id.clone())
                .or_default()
                .push(message_type.clone());
        }
        for (message_id, message_types) in types_by_id {
            if message_types.len() > 1 {
                return Err(DuckDbAdapterError::AmbiguousMessageType {
                    message_id,
                    message_types,
                });
            }
        }

        Ok(headers)
    }

    pub fn read_render_rows(
        &self,
        layout: &DatabaseLayout,
        message_id: &str,
    ) -> Result<Vec<RenderRow>, DuckDbAdapterError> {
        let mut rows = Vec::new();

        for table in layout.tables.iter().filter(|table| {
            !table.name.starts_with("swift_")
                && table
                    .columns
                    .iter()
                    .any(|column| column.name == "message_id")
                && table
                    .columns
                    .iter()
                    .any(|column| column.name == "sequence_path")
        }) {
            rows.extend(self.read_render_rows_for_table(table, message_id)?);
        }

        Ok(rows)
    }

    fn read_render_rows_for_table(
        &self,
        table: &TableLayout,
        message_id: &str,
    ) -> Result<Vec<RenderRow>, DuckDbAdapterError> {
        let projections = table
            .columns
            .iter()
            .map(|column| {
                format!(
                    "CAST({} AS VARCHAR) AS {}",
                    ident(&column.name),
                    ident(&column.name)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT {} FROM {} WHERE {} = ? ORDER BY {}",
            projections,
            ident(&table.name),
            ident("message_id"),
            ident("sequence_path")
        );
        let column_names = table
            .columns
            .iter()
            .map(|column| column.name.clone())
            .collect::<Vec<_>>();
        let mut stmt = self.conn.prepare(&sql)?;
        let result = stmt.query_map([message_id], |row| {
            let mut values = std::collections::BTreeMap::new();
            for (index, column) in column_names.iter().enumerate() {
                let value: Option<String> = row.get(index)?;
                if let Some(value) = value {
                    values.insert(column.clone(), value);
                }
            }
            Ok(RenderRow {
                table: table.name.clone(),
                values,
            })
        })?;

        let mut rows = Vec::new();
        for row in result {
            rows.push(row?);
        }
        Ok(rows)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuckDbInboundConfig {
    pub inbound_table: String,
    pub id_column: String,
    pub message_type_column: String,
    pub body_column: String,
    pub processed_column: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuckDbExportFormat {
    Csv,
    Parquet,
}

impl DuckDbExportFormat {
    fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Parquet => "parquet",
        }
    }

    fn duckdb_name(self) -> &'static str {
        match self {
            Self::Csv => "CSV",
            Self::Parquet => "PARQUET",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DuckDbExportOptions {
    pub parquet_row_group_size: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportedTable {
    pub table: String,
    pub path: std::path::PathBuf,
    pub format: DuckDbExportFormat,
}

impl Default for DuckDbInboundConfig {
    fn default() -> Self {
        Self {
            inbound_table: "inbound_messages".to_string(),
            id_column: "id".to_string(),
            message_type_column: "message_type".to_string(),
            body_column: "body".to_string(),
            processed_column: Some("processed".to_string()),
        }
    }
}

#[derive(Debug)]
pub enum DuckDbAdapterError {
    DuckDb(duckdb::Error),
    Io(std::io::Error),
    UnknownNormalizedTable {
        table: String,
    },
    MissingNormalizedColumn {
        table: String,
        column: String,
    },
    MessageTypeNotFound {
        message_id: String,
    },
    AmbiguousMessageType {
        message_id: String,
        message_types: Vec<String>,
    },
}

impl fmt::Display for DuckDbAdapterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuckDb(error) => write!(f, "duckdb error: {error}"),
            Self::Io(error) => write!(f, "io error: {error}"),
            Self::UnknownNormalizedTable { table } => {
                write!(f, "normalized table is not present in layout: {table}")
            }
            Self::MissingNormalizedColumn { table, column } => {
                write!(
                    f,
                    "normalized row for table {table} is missing grouped column {column}"
                )
            }
            Self::MessageTypeNotFound { message_id } => {
                write!(f, "message type not found for message id {message_id}")
            }
            Self::AmbiguousMessageType {
                message_id,
                message_types,
            } => {
                write!(
                    f,
                    "ambiguous message type for message id {message_id}: {}",
                    message_types.join(", ")
                )
            }
        }
    }
}

impl std::error::Error for DuckDbAdapterError {}

impl From<duckdb::Error> for DuckDbAdapterError {
    fn from(value: duckdb::Error) -> Self {
        Self::DuckDb(value)
    }
}

impl From<std::io::Error> for DuckDbAdapterError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl InboundSource for DuckDbStore {
    type Error = DuckDbAdapterError;

    fn read_batch(&mut self, limit: usize) -> Result<MessageBatch, Self::Error> {
        let processed_filter = self
            .processed_column
            .as_ref()
            .map(|column| format!(" WHERE COALESCE({}, false) = false", ident(column)))
            .unwrap_or_default();
        let sql = format!(
            "SELECT {}, {}, {} FROM {}{} ORDER BY {} LIMIT {}",
            ident(&self.id_column),
            ident(&self.message_type_column),
            ident(&self.body_column),
            ident(&self.inbound_table),
            processed_filter,
            ident(&self.id_column),
            limit,
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], |row| {
            Ok(InboundMessage {
                id: row.get(0)?,
                message_type: row.get(1)?,
                body: row.get(2)?,
            })
        })?;

        let mut messages = Vec::new();
        for row in rows {
            messages.push(row?);
        }

        Ok(MessageBatch { messages })
    }

    fn mark_processed(&mut self, ids: &[String]) -> Result<(), Self::Error> {
        let Some(processed_column) = &self.processed_column else {
            return Ok(());
        };

        let sql = format!(
            "UPDATE {} SET {} = true WHERE {} = ?",
            ident(&self.inbound_table),
            ident(processed_column),
            ident(&self.id_column)
        );
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare(&sql)?;
            for id in ids {
                stmt.execute([id])?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}

impl MigrationSink for DuckDbStore {
    type Error = DuckDbAdapterError;

    fn apply_layout(&mut self, layout: &DatabaseLayout) -> Result<(), Self::Error> {
        for table in &layout.tables {
            self.conn.execute(&create_table_sql(table), [])?;
            add_missing_columns(&self.conn, table)?;
        }
        Ok(())
    }
}

impl ParsedSink for DuckDbStore {
    type Error = DuckDbAdapterError;

    fn write_batch(&mut self, batch: &ParsedOutputBatch) -> Result<(), Self::Error> {
        let tx = self.conn.transaction()?;

        insert_raw_messages(&tx, &batch.raw_messages, self.write_batch_size)?;
        insert_fields(&tx, &batch.fields, self.write_batch_size)?;
        insert_parse_errors(&tx, &batch.parse_errors, self.write_batch_size)?;
        insert_normalized_rows(&tx, &batch.normalized_rows, self.write_batch_size)?;

        tx.commit()?;
        Ok(())
    }
}

fn insert_raw_messages(
    conn: &Connection,
    rows: &[RawMessageRow],
    write_batch_size: usize,
) -> Result<(), DuckDbAdapterError> {
    if rows.is_empty() {
        return Ok(());
    }

    let mut stmt = conn.prepare(
        "INSERT INTO swift_raw_messages (message_id, message_type, raw_text) VALUES (?, ?, ?)",
    )?;
    for chunk in rows.chunks(write_batch_size) {
        for row in chunk {
            stmt.execute([&row.message_id, &row.message_type, &row.raw_text])?;
        }
    }
    Ok(())
}

fn insert_fields(
    conn: &Connection,
    rows: &[FieldRow],
    write_batch_size: usize,
) -> Result<(), DuckDbAdapterError> {
    if rows.is_empty() {
        return Ok(());
    }

    let mut stmt = conn.prepare(
        "INSERT INTO swift_fields (message_id, sequence_path, tag, qualifier, raw_value) VALUES (?, ?, ?, ?, ?)",
    )?;
    for chunk in rows.chunks(write_batch_size) {
        for row in chunk {
            stmt.execute((
                &row.message_id,
                row.sequence_path.as_deref(),
                &row.tag,
                row.qualifier.as_deref(),
                &row.raw_value,
            ))?;
        }
    }
    Ok(())
}

fn insert_parse_errors(
    conn: &Connection,
    rows: &[ParseErrorRow],
    write_batch_size: usize,
) -> Result<(), DuckDbAdapterError> {
    if rows.is_empty() {
        return Ok(());
    }

    let mut stmt =
        conn.prepare("INSERT INTO swift_parse_errors (message_id, error) VALUES (?, ?)")?;
    for chunk in rows.chunks(write_batch_size) {
        for row in chunk {
            stmt.execute([&row.message_id, &row.error])?;
        }
    }
    Ok(())
}

fn insert_normalized_rows(
    conn: &Connection,
    rows: &[NormalizedRow],
    write_batch_size: usize,
) -> Result<(), DuckDbAdapterError> {
    let mut groups = BTreeMap::<(String, Vec<String>), Vec<&NormalizedRow>>::new();
    for row in rows {
        let columns = row.values.keys().cloned().collect::<Vec<_>>();
        groups
            .entry((row.table.clone(), columns))
            .or_default()
            .push(row);
    }

    for ((table, columns), rows) in groups {
        insert_normalized_group(conn, &table, &columns, &rows, write_batch_size)?;
    }

    Ok(())
}

fn insert_normalized_group(
    conn: &Connection,
    table: &str,
    columns: &[String],
    rows: &[&NormalizedRow],
    write_batch_size: usize,
) -> Result<(), DuckDbAdapterError> {
    if rows.is_empty() {
        return Ok(());
    }

    let columns_sql = columns
        .iter()
        .map(|column| ident(column))
        .collect::<Vec<_>>();
    let placeholders = vec!["?"; columns.len()].join(", ");
    let sql = format!(
        "INSERT INTO {} ({}) VALUES ({})",
        ident(table),
        columns_sql.join(", "),
        placeholders
    );
    let mut stmt = conn.prepare(&sql)?;

    for chunk in rows.chunks(write_batch_size) {
        for row in chunk {
            let values = columns
                .iter()
                .map(|column| {
                    row.values.get(column).map_or_else(
                        || {
                            Err(DuckDbAdapterError::MissingNormalizedColumn {
                                table: table.to_string(),
                                column: column.clone(),
                            })
                        },
                        |value| Ok(value as &dyn ToSql),
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            stmt.execute(params_from_iter(values))?;
        }
    }

    Ok(())
}

fn create_table_sql(table: &TableLayout) -> String {
    let columns = table
        .columns
        .iter()
        .map(column_sql)
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "CREATE TABLE IF NOT EXISTS {} ({})",
        ident(&table.name),
        columns
    )
}

fn add_missing_columns(conn: &Connection, table: &TableLayout) -> Result<(), DuckDbAdapterError> {
    let existing = existing_columns(conn, &table.name)?;
    for column in &table.columns {
        if existing.contains(&column.name) {
            continue;
        }
        let sql = format!(
            "ALTER TABLE {} ADD COLUMN {} {}",
            ident(&table.name),
            ident(&column.name),
            logical_type_sql(column.logical_type)
        );
        conn.execute(&sql, [])?;
    }
    Ok(())
}

fn existing_columns(
    conn: &Connection,
    table_name: &str,
) -> Result<BTreeSet<String>, DuckDbAdapterError> {
    let mut stmt =
        conn.prepare("SELECT column_name FROM information_schema.columns WHERE table_name = ?")?;
    let rows = stmt.query_map([table_name], |row| row.get::<_, String>(0))?;
    let mut columns = BTreeSet::new();
    for row in rows {
        columns.insert(row?);
    }
    Ok(columns)
}

fn column_sql(column: &ColumnLayout) -> String {
    let nullability = if column.required { " NOT NULL" } else { "" };
    format!(
        "{} {}{}",
        ident(&column.name),
        logical_type_sql(column.logical_type),
        nullability
    )
}

fn logical_type_sql(logical_type: LogicalColumnType) -> &'static str {
    match logical_type {
        LogicalColumnType::Text => "TEXT",
        LogicalColumnType::Date => "DATE",
        LogicalColumnType::Decimal => "DECIMAL(38, 12)",
        LogicalColumnType::Json => "JSON",
    }
}

fn ident(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn sql_string(value: &str) -> String {
    value.replace('\'', "''")
}

fn copy_options_sql(format: DuckDbExportFormat, options: DuckDbExportOptions) -> String {
    let mut parts = vec![format!("FORMAT {}", format.duckdb_name())];
    if let (DuckDbExportFormat::Parquet, Some(row_group_size)) =
        (format, options.parquet_row_group_size)
    {
        parts.push(format!("ROW_GROUP_SIZE {}", row_group_size.max(1)));
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::too_many_lines,
        reason = "integration-style fixtures are clearer as complete scenarios"
    )]

    use super::*;
    use duckdb::Connection;
    use swift_core::parse_message;
    use swift_db::{
        materialize_message, InboundSource, MigrationSink, ParsedOutputBatch, ParsedSink,
    };
    use swift_schema::{
        infer_database_layout, match_and_parse_message, render_message, RenderEnvelope,
        RenderRequest, SchemaCatalog,
    };

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
    sequences:
      GENL: {}
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

    #[test]
    fn builds_copy_options_for_parquet_row_group_size() {
        let options = DuckDbExportOptions {
            parquet_row_group_size: Some(50_000),
        };

        assert_eq!(
            copy_options_sql(DuckDbExportFormat::Parquet, options),
            "FORMAT PARQUET, ROW_GROUP_SIZE 50000"
        );
        assert_eq!(
            copy_options_sql(DuckDbExportFormat::Csv, options),
            "FORMAT CSV"
        );
    }

    #[test]
    fn reads_inbound_migrates_and_writes_parsed_output() {
        let conn = Connection::open_in_memory().expect("opens duckdb");
        conn.execute(
            "CREATE TABLE inbound_messages (
                id TEXT NOT NULL,
                message_type TEXT NOT NULL,
                body TEXT NOT NULL,
                processed BOOLEAN NOT NULL DEFAULT false
            )",
            [],
        )
        .expect("creates inbound");
        conn.execute(
            "INSERT INTO inbound_messages (id, message_type, body) VALUES (?, ?, ?)",
            [
                "msg-1",
                "MT540",
                "{1:F01BANKBEBBAXXX0000000000}{2:I540BANKDEFFXXXXN}{4:\n:16R:GENL\n:20C::SEME//ABC123\n:98A::PREP//20260511\n:16R:LINK\n:20C::RELA//REL1\n:16S:LINK\n:16S:GENL\n-}",
            ],
        )
        .expect("inserts inbound");

        let catalog = SchemaCatalog::from_yaml_str(SCHEMA).expect("schema loads");
        catalog.validate().expect("schema validates");
        let layout = infer_database_layout(&catalog);
        let mut store = DuckDbStore::new(conn, DuckDbInboundConfig::default());

        store.apply_layout(&layout).expect("migrates");
        let inbound = store.read_batch(100).expect("reads inbound");
        assert_eq!(inbound.messages.len(), 1);

        let message = &inbound.messages[0];
        let parsed = parse_message(message.body.as_bytes());
        let output = materialize_message(&catalog, message, &parsed).expect("materializes");
        store.write_batch(&output).expect("writes output");
        store
            .mark_processed(std::slice::from_ref(&message.id))
            .expect("marks processed");

        let conn = store.connection();
        let raw_count: i64 = conn
            .query_row("SELECT count(*) FROM swift_raw_messages", [], |row| {
                row.get(0)
            })
            .expect("raw count");
        let field_count: i64 = conn
            .query_row("SELECT count(*) FROM swift_fields", [], |row| row.get(0))
            .expect("field count");
        let sender_reference: String = conn
            .query_row(
                "SELECT sender_reference FROM settlement_instruction WHERE message_id = 'msg-1'",
                [],
                |row| row.get(0),
            )
            .expect("sender reference");
        let related_reference: String = conn
            .query_row(
                "SELECT related_reference FROM message_reference WHERE message_id = 'msg-1'",
                [],
                |row| row.get(0),
            )
            .expect("related reference");
        let processed: bool = conn
            .query_row(
                "SELECT processed FROM inbound_messages WHERE id = 'msg-1'",
                [],
                |row| row.get(0),
            )
            .expect("processed");

        assert_eq!(raw_count, 1);
        assert_eq!(field_count, 7);
        assert_eq!(sender_reference, "ABC123");
        assert_eq!(related_reference, "REL1");
        assert!(processed);
    }

    #[test]
    fn rejects_ambiguous_message_type_lookup() {
        let conn = Connection::open_in_memory().expect("opens duckdb");
        conn.execute(
            "CREATE TABLE swift_raw_messages (
                message_id TEXT NOT NULL,
                message_type TEXT NOT NULL,
                raw_message TEXT NOT NULL
            )",
            [],
        )
        .expect("creates raw messages");
        conn.execute(
            "INSERT INTO swift_raw_messages (message_id, message_type, raw_message) VALUES
                ('msg-1', 'MT540', 'raw-a'),
                ('msg-1', 'MT541', 'raw-b')",
            [],
        )
        .expect("inserts ambiguous raw messages");

        let store = DuckDbStore::new(conn, DuckDbInboundConfig::default());
        let error = store
            .read_message_type("msg-1")
            .expect_err("ambiguous message type should fail");

        assert!(matches!(
            error,
            DuckDbAdapterError::AmbiguousMessageType {
                ref message_id,
                ref message_types,
            } if message_id == "msg-1"
                && message_types == &vec!["MT540".to_string(), "MT541".to_string()]
        ));
    }

    #[test]
    fn rejects_ambiguous_batch_render_headers() {
        let conn = Connection::open_in_memory().expect("opens duckdb");
        conn.execute(
            "CREATE TABLE swift_raw_messages (
                message_id TEXT NOT NULL,
                message_type TEXT NOT NULL,
                raw_message TEXT NOT NULL
            )",
            [],
        )
        .expect("creates raw messages");
        conn.execute(
            "INSERT INTO swift_raw_messages (message_id, message_type, raw_message) VALUES
                ('msg-1', 'MT540', 'raw-a'),
                ('msg-1', 'MT541', 'raw-b')",
            [],
        )
        .expect("inserts ambiguous raw messages");

        let store = DuckDbStore::new(conn, DuckDbInboundConfig::default());
        let error = store
            .read_render_message_headers()
            .expect_err("ambiguous render headers should fail");

        assert!(matches!(
            error,
            DuckDbAdapterError::AmbiguousMessageType {
                ref message_id,
                ref message_types,
            } if message_id == "msg-1"
                && message_types == &vec!["MT540".to_string(), "MT541".to_string()]
        ));
    }

    #[test]
    fn reads_normalized_rows_and_renders_parseable_fin_message() {
        let conn = Connection::open_in_memory().expect("opens duckdb");
        conn.execute(
            "CREATE TABLE inbound_messages (
                id TEXT NOT NULL,
                message_type TEXT NOT NULL,
                body TEXT NOT NULL,
                processed BOOLEAN NOT NULL DEFAULT false
            )",
            [],
        )
        .expect("creates inbound");
        conn.execute(
            "INSERT INTO inbound_messages (id, message_type, body) VALUES (?, ?, ?)",
            [
                "msg-1",
                "MT540",
                "{1:F01BANKBEBBAXXX0000000000}{2:I540BANKDEFFXXXXN}{4:\n:16R:GENL\n:20C::SEME//ABC123\n:98A::PREP//20260511\n:16R:LINK\n:20C::RELA//REL1\n:16S:LINK\n:16S:GENL\n-}",
            ],
        )
        .expect("inserts inbound");

        let catalog = SchemaCatalog::from_yaml_str(SCHEMA).expect("schema loads");
        catalog.validate().expect("schema validates");
        let layout = infer_database_layout(&catalog);
        let mut store = DuckDbStore::new(conn, DuckDbInboundConfig::default());
        store.apply_layout(&layout).expect("migrates");

        let inbound = store.read_batch(100).expect("reads inbound");
        let message = &inbound.messages[0];
        let parsed = parse_message(message.body.as_bytes());
        let output = materialize_message(&catalog, message, &parsed).expect("materializes");
        store.write_batch(&output).expect("writes output");

        let rows = store
            .read_render_rows(&layout, "msg-1")
            .expect("reads render rows");
        let rendered = render_message(
            &catalog,
            &RenderRequest {
                message_id: "msg-1".to_string(),
                message_type: "MT540".to_string(),
                envelope: RenderEnvelope {
                    block1: "F01BANKBEBBAXXX0000000000".to_string(),
                    block2: "I540BANKDEFFXXXXN".to_string(),
                    block3: None,
                    block5: None,
                },
                rows,
            },
        )
        .expect("renders");
        let parsed_rendered = parse_message(rendered.as_bytes());
        assert!(parsed_rendered.diagnostics.is_empty());
        let schema = catalog.message("MT540").expect("MT540 schema");
        let matched = match_and_parse_message(&catalog, schema, &parsed_rendered);
        assert!(matched.parse_errors.is_empty());
        assert!(matched.missing_required.is_empty());
        assert!(matched.cardinality_violations.is_empty());
        assert!(matched.sequence_issues.is_empty());
    }

    #[test]
    fn renders_repeated_sequence_occurrences_from_duckdb_without_interleaving_fields() {
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
"#;
        let conn = Connection::open_in_memory().expect("opens duckdb");
        conn.execute(
            "CREATE TABLE inbound_messages (
                id TEXT NOT NULL,
                message_type TEXT NOT NULL,
                body TEXT NOT NULL,
                processed BOOLEAN NOT NULL DEFAULT false
            )",
            [],
        )
        .expect("creates inbound");
        conn.execute(
            "INSERT INTO inbound_messages (id, message_type, body) VALUES (?, ?, ?)",
            [
                "msg-1",
                "MT537",
                "{1:F01BANKBEBBAXXX0000000000}{2:I537BANKDEFFXXXXN}{4:\n:16R:STAT\n:16R:TRAN\n:20C::RELA//REL1\n:25D::SETT//PEND\n:16S:TRAN\n:16R:TRAN\n:20C::RELA//REL2\n:25D::SETT//SETT\n:16S:TRAN\n:16S:STAT\n-}",
            ],
        )
        .expect("inserts inbound");

        let catalog = SchemaCatalog::from_yaml_str(schema).expect("schema loads");
        catalog.validate().expect("schema validates");
        let layout = infer_database_layout(&catalog);
        let mut store = DuckDbStore::new(conn, DuckDbInboundConfig::default());
        store.apply_layout(&layout).expect("migrates");

        let inbound = store.read_batch(100).expect("reads inbound");
        let parsed = parse_message(inbound.messages[0].body.as_bytes());
        let output =
            materialize_message(&catalog, &inbound.messages[0], &parsed).expect("materializes");
        assert_eq!(output.normalized_rows.len(), 2);
        store.write_batch(&output).expect("writes output");

        let rows = store
            .read_render_rows(&layout, "msg-1")
            .expect("reads render rows");
        let rendered = render_message(
            &catalog,
            &RenderRequest {
                message_id: "msg-1".to_string(),
                message_type: "MT537".to_string(),
                envelope: RenderEnvelope {
                    block1: "F01BANKBEBBAXXX0000000000".to_string(),
                    block2: "I537BANKDEFFXXXXN".to_string(),
                    block3: None,
                    block5: None,
                },
                rows,
            },
        )
        .expect("renders");

        let first_transaction = rendered
            .find(":20C::RELA//REL1\n:25D::SETT//PEND\n:16S:TRAN\n:16R:TRAN\n:20C::RELA//REL2")
            .expect("first transaction is closed before second starts");
        let second_status = rendered
            .find(":20C::RELA//REL2\n:25D::SETT//SETT")
            .expect("second transaction fields stay together");
        assert!(first_transaction < second_status);

        let parsed_rendered = parse_message(rendered.as_bytes());
        assert!(parsed_rendered.diagnostics.is_empty());
        let message_schema = catalog.message("MT537").expect("message schema");
        let matched = match_and_parse_message(&catalog, message_schema, &parsed_rendered);
        assert!(matched.parse_errors.is_empty());
        assert!(matched.missing_required.is_empty());
        assert!(matched.cardinality_violations.is_empty());
        assert!(matched.sequence_issues.is_empty());
    }

    #[test]
    fn parses_exports_and_validates_all_five_example_message_types() {
        let conn = Connection::open_in_memory().expect("opens duckdb");
        conn.execute(
            "CREATE TABLE inbound_messages (
                id TEXT NOT NULL,
                message_type TEXT NOT NULL,
                body TEXT NOT NULL,
                processed BOOLEAN NOT NULL DEFAULT false
            )",
            [],
        )
        .expect("creates inbound");

        for (id, message_type, file_name) in [
            ("msg-537", "MT537", "mt537_sample.fin"),
            ("msg-540", "MT540", "mt540_sample.fin"),
            ("msg-541", "MT541", "mt541_sample.fin"),
            ("msg-542", "MT542", "mt542_sample.fin"),
            ("msg-543", "MT543", "mt543_sample.fin"),
        ] {
            let body = read_example(file_name);
            conn.execute(
                "INSERT INTO inbound_messages (id, message_type, body) VALUES (?, ?, ?)",
                [id, message_type, body.as_str()],
            )
            .expect("inserts inbound fixture");
        }

        let catalog = example_catalog();
        catalog.validate().expect("schema validates");
        let layout = infer_database_layout(&catalog);
        let mut store = DuckDbStore::new(conn, DuckDbInboundConfig::default());
        store.apply_layout(&layout).expect("migrates");

        let inbound = store.read_batch(100).expect("reads inbound");
        assert_eq!(inbound.messages.len(), 5);

        let mut output = ParsedOutputBatch::empty();
        let mut processed_ids = Vec::new();
        for message in &inbound.messages {
            let parsed = parse_message(message.body.as_bytes());
            let message_output =
                materialize_message(&catalog, message, &parsed).expect("materializes");
            output.extend(message_output);
            processed_ids.push(message.id.clone());
        }

        assert_eq!(output.raw_messages.len(), 5);
        assert_eq!(output.fields.len(), 119);
        assert_eq!(output.normalized_rows.len(), 35);
        assert_eq!(output.parse_errors.len(), 0);

        store.write_batch(&output).expect("writes output");
        store
            .mark_processed(&processed_ids)
            .expect("marks processed");
        store
            .connection()
            .execute(
                "UPDATE settlement_indicator SET method = 'RECE' WHERE message_id = 'msg-540'",
                [],
            )
            .expect("edits settlement method");

        let conn = store.connection();
        assert_count(conn, "swift_raw_messages", 5);
        assert_count(conn, "swift_fields", 119);
        assert_count(conn, "swift_parse_errors", 0);
        assert_count(conn, "mt537_statement", 1);
        assert_count(conn, "settlement_instruction", 4);
        assert_count(conn, "settlement_trade", 4);
        assert_count(conn, "settlement_quantity", 4);

        for (message_id, message_type) in [
            ("msg-537", "MT537"),
            ("msg-540", "MT540"),
            ("msg-541", "MT541"),
            ("msg-542", "MT542"),
            ("msg-543", "MT543"),
        ] {
            let rows = store
                .read_render_rows(&layout, message_id)
                .expect("reads render rows");
            let rendered = render_message(
                &catalog,
                &RenderRequest {
                    message_id: message_id.to_string(),
                    message_type: message_type.to_string(),
                    envelope: RenderEnvelope {
                        block1: "F01BANKBEBBAXXX0000000000".to_string(),
                        block2: format!("I{}BANKDEFFXXXXN", message_type.trim_start_matches("MT")),
                        block3: None,
                        block5: None,
                    },
                    rows,
                },
            )
            .unwrap_or_else(|error| panic!("{message_type} should render: {error}"));
            if message_type == "MT540" {
                assert!(rendered.contains(":36B::SETT//UNIT/1000,"));
                assert!(rendered.contains(":22F::STCO//NOMC"));
                assert!(rendered.contains(":22H::REDE//RECE"));
            }
            let parsed_rendered = parse_message(rendered.as_bytes());
            assert!(
                parsed_rendered.diagnostics.is_empty(),
                "{message_type} rendered with diagnostics: {:?}",
                parsed_rendered.diagnostics
            );
            let schema = catalog.message(message_type).expect("message schema");
            let matched = match_and_parse_message(&catalog, schema, &parsed_rendered);
            assert!(
                matched.parse_errors.is_empty(),
                "{message_type} rendered with parse errors: {:?}",
                matched.parse_errors
            );
            assert!(
                matched.missing_required.is_empty(),
                "{message_type} rendered missing required fields: {:?}",
                matched.missing_required
            );
            assert!(
                matched.cardinality_violations.is_empty(),
                "{message_type} rendered cardinality violations: {:?}",
                matched.cardinality_violations
            );
            assert!(
                matched.sequence_issues.is_empty(),
                "{message_type} rendered sequence issues: {:?}",
                matched.sequence_issues
            );
        }

        let export_dir = unique_temp_dir("swiftpipe-all5-export");
        let exported = store
            .export_layout(&layout, &export_dir, &[DuckDbExportFormat::Csv])
            .expect("exports layout");
        assert!(exported.iter().any(|item| item.table == "swift_fields"));
        assert!(export_dir.join("swift_fields.csv").is_file());
        assert!(export_dir.join("settlement_instruction.csv").is_file());
    }

    #[test]
    fn parses_renders_and_validates_all_local_example_message_types() {
        let cases = local_example_cases();
        assert_eq!(cases.len(), 62, "expected the local 62-sample corpus");

        let conn = Connection::open_in_memory().expect("opens duckdb");
        conn.execute(
            "CREATE TABLE inbound_messages (
                id TEXT NOT NULL,
                message_type TEXT NOT NULL,
                body TEXT NOT NULL,
                processed BOOLEAN NOT NULL DEFAULT false
            )",
            [],
        )
        .expect("creates inbound");

        for (message_type, sample_file, _) in &cases {
            let body = read_example(sample_file);
            let id = message_id(message_type);
            conn.execute(
                "INSERT INTO inbound_messages (id, message_type, body) VALUES (?, ?, ?)",
                [id.as_str(), message_type.as_str(), body.as_str()],
            )
            .expect("inserts inbound fixture");
        }

        let catalog = all_example_catalog();
        catalog.validate().expect("schema validates");
        let layout = infer_database_layout(&catalog);
        let mut store = DuckDbStore::new(conn, DuckDbInboundConfig::default());
        store.apply_layout(&layout).expect("migrates");

        let inbound = store.read_batch(100).expect("reads inbound");
        assert_eq!(inbound.messages.len(), cases.len());

        let mut output = ParsedOutputBatch::empty();
        let mut processed_ids = Vec::new();
        for message in &inbound.messages {
            // Some message types (MT940/942/950) repeat a field group with no
            // :16R:/:16S: wrapper (ADR-0013) — their schema declares this as
            // an anchored sequence. A no-op for every other type here.
            let anchored = catalog
                .message(&message.message_type)
                .map(swift_schema::anchored_sequences)
                .unwrap_or_default();
            let parsed = swift_core::parse_message_with_sequences(message.body.as_bytes(), &anchored);
            assert!(
                parsed.diagnostics.is_empty(),
                "{} sample should parse without diagnostics: {:?}",
                message.message_type,
                parsed.diagnostics
            );
            let message_output =
                materialize_message(&catalog, message, &parsed).expect("materializes");
            assert!(
                message_output.parse_errors.is_empty(),
                "{} sample should materialize without schema errors: {:?}",
                message.message_type,
                message_output.parse_errors
            );
            output.extend(message_output);
            processed_ids.push(message.id.clone());
        }

        store.write_batch(&output).expect("writes output");
        store
            .mark_processed(&processed_ids)
            .expect("marks processed");

        for (message_type, _, _) in &cases {
            let id = message_id(message_type);
            let rows = store
                .read_render_rows(&layout, &id)
                .expect("reads render rows");
            let rendered = render_message(
                &catalog,
                &RenderRequest {
                    message_id: id,
                    message_type: message_type.clone(),
                    envelope: RenderEnvelope {
                        block1: "F01BANKBEBBAXXX0000000000".to_string(),
                        block2: format!("I{}BANKDEFFXXXXN", message_type.trim_start_matches("MT")),
                        block3: None,
                        block5: None,
                    },
                    rows,
                },
            )
            .unwrap_or_else(|error| panic!("{message_type} should render: {error}"));

            let schema = catalog.message(message_type).expect("message schema");
            let anchored = swift_schema::anchored_sequences(schema);
            let parsed_rendered =
                swift_core::parse_message_with_sequences(rendered.as_bytes(), &anchored);
            assert!(
                parsed_rendered.diagnostics.is_empty(),
                "{message_type} rendered with diagnostics: {:?}",
                parsed_rendered.diagnostics
            );
            let matched = match_and_parse_message(&catalog, schema, &parsed_rendered);
            assert!(
                matched.parse_errors.is_empty(),
                "{message_type} rendered with parse errors: {:?}",
                matched.parse_errors
            );
            assert!(
                matched.missing_required.is_empty(),
                "{message_type} rendered missing required fields: {:?}",
                matched.missing_required
            );
            assert!(
                matched.cardinality_violations.is_empty(),
                "{message_type} rendered cardinality violations: {:?}",
                matched.cardinality_violations
            );
            assert!(
                matched.sequence_issues.is_empty(),
                "{message_type} rendered sequence issues: {:?}",
                matched.sequence_issues
            );
        }
    }

    fn example_catalog() -> SchemaCatalog {
        catalog_from_schema_files(&[
            "mt537.yaml",
            "mt540.yaml",
            "mt541.yaml",
            "mt542.yaml",
            "mt543.yaml",
        ])
    }

    fn all_example_catalog() -> SchemaCatalog {
        let files = local_example_cases()
            .into_iter()
            .map(|(_, _, schema_file)| schema_file)
            .collect::<Vec<_>>();
        catalog_from_schema_files(&files)
    }

    fn catalog_from_schema_files<S: AsRef<str>>(file_names: &[S]) -> SchemaCatalog {
        let mut catalog = SchemaCatalog {
            field_types: Vec::new(),
            messages: Vec::new(),
        };
        for file_name in file_names {
            let content = read_example_schema(file_name.as_ref());
            let mut partial = SchemaCatalog::from_yaml_str(&content).expect("schema loads");
            for field_type in partial.field_types.drain(..) {
                if !catalog
                    .field_types
                    .iter()
                    .any(|existing| existing == &field_type)
                {
                    catalog.field_types.push(field_type);
                }
            }
            catalog.messages.append(&mut partial.messages);
        }
        catalog
    }

    fn local_example_cases() -> Vec<(String, String, String)> {
        let mut cases = Vec::new();
        for entry in std::fs::read_dir(example_path("")).expect("reads examples directory") {
            let entry = entry.expect("reads examples directory entry");
            let file_name = entry.file_name();
            let Some(file_name) = file_name.to_str() else {
                continue;
            };
            let Some(mt) = file_name
                .strip_prefix("mt")
                .and_then(|value| value.strip_suffix("_sample.fin"))
            else {
                continue;
            };
            let message_type = format!("MT{mt}");
            let schema_file = format!("mt{mt}.yaml");
            if example_path(&format!("schemas/{schema_file}")).is_file() {
                cases.push((message_type, file_name.to_string(), schema_file));
            }
        }
        cases.sort_by(|left, right| left.0.cmp(&right.0));
        cases
    }

    fn message_id(message_type: &str) -> String {
        format!(
            "msg-{}",
            message_type.trim_start_matches("MT").to_ascii_lowercase()
        )
    }

    fn read_example(file_name: &str) -> String {
        std::fs::read_to_string(example_path(file_name)).expect("reads example fixture")
    }

    fn read_example_schema(file_name: &str) -> String {
        std::fs::read_to_string(example_path(&format!("schemas/{file_name}")))
            .expect("reads example schema")
    }

    fn example_path(file_name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples")
            .join(file_name)
    }

    fn unique_temp_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("{}-{}", name, std::process::id()))
    }

    fn assert_count(conn: &Connection, table: &str, expected: i64) {
        let sql = format!("SELECT count(*) FROM {}", ident(table));
        let actual: i64 = conn
            .query_row(&sql, [], |row| row.get(0))
            .expect("queries table count");
        assert_eq!(actual, expected, "unexpected count for {table}");
    }
}
