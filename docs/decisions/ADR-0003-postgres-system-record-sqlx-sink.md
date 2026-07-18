# ADR-0003: Use SQLx For The Postgres System-Of-Record Sink

## Status

Accepted

## Context

SwiftPipe has two separate relational surfaces:

- per-job DuckDB hydration for normalized output and Parquet artifact creation
- optional system-of-record sync for job, audit, and artifact metadata

The system-of-record path must write durable control-plane metadata without
coupling metadata durability to per-job DuckDB artifact databases. It also needs
readiness checks, connection pooling, schema creation, and clear failure
messages for deployment operators.

The implemented code uses SQLx for Postgres and reserves DuckDB for artifact
hydration:

- `repo/crates/swift-api/src/system_record.rs` defines
  `SystemOfRecordConfig::Postgres`, `ReadyCheck::postgres`, and
  `SqlxPostgresSink`.
- `SqlxPostgresSink::connect` creates a `sqlx::PgPool`, runs schema/table
  creation, then upserts jobs and writes audit/artifact rows.
- `repo/crates/swift-api/Cargo.toml` enables SQLx with the `postgres` feature.
- `repo/crates/swift-api/src/job.rs` uses `DuckDbStore` only when selected
  outputs require relational hydration, such as Parquet.
- `repo/crates/swift-api/src/system_record.rs` keeps SQL Server as an explicit
  unsupported stub until a separate adapter is implemented.

## Decision

Use SQLx as the Postgres system-of-record client and keep DuckDB scoped to
per-job artifact hydration/export.

Postgres system-of-record mode writes the `swiftpipe_jobs`,
`swiftpipe_audit_events`, and `swiftpipe_artifacts` tables through SQLx. DuckDB
remains the embedded SQL surface for normalized batch hydration and Parquet
exports, not the bridge to Postgres metadata durability.

## Consequences

- Postgres readiness can use a direct `SELECT 1` against a SQLx pool.
- The system-of-record sink avoids loading DuckDB extensions or attaching a
  remote database per job.
- DuckDB artifact behavior remains independent from control-plane metadata
  durability.
- SQL Server remains deferred behind an explicit unsupported mode rather than
  sharing the Postgres implementation path.
- The Postgres sink still needs production hardening around long-lived pooling,
  crash recovery, and transaction boundaries.

## Alternatives Considered

- Use DuckDB's Postgres extension as the metadata bridge. Rejected for the
  current implementation because it couples control-plane durability to an
  artifact-hydration engine and requires extension/attach lifecycle management.
- Write system-of-record data only to JSONL. Rejected as the only production
  path because operators need queryable durable metadata in Postgres.
- Implement SQL Server alongside Postgres now. Rejected because SQL Server needs
  a separate ODBC/nanodbc adapter and should not block the Postgres path.
