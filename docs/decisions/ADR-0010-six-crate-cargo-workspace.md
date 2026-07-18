# ADR-0010: Keep The Six-Crate Cargo Workspace

## Status

Accepted

## Context

SwiftPipe has distinct concerns that need separate ownership boundaries:
structural FIN parsing, schema loading/matching/rendering, database-neutral
materialization contracts, DuckDB persistence/export, CLI workflows, and the
self-hosted API.

The implemented workspace has six crates:

- `repo/crates/swift-core`: zero-copy SWIFT FIN structural parser.
- `repo/crates/swift-schema`: YAML schema loading, validation, matching, layout
  inference, and rendering.
- `repo/crates/swift-db`: database-neutral ingestion contracts and output
  materialization.
- `repo/crates/swift-duckdb`: DuckDB adapter for parsed output, render reads,
  and export.
- `repo/crates/swift-cli`: command-line schema, migrate, run, render, and
  export workflows.
- `repo/crates/swift-api`: HTTP API, auth, queueing, object store, metrics,
  system-of-record sync, and embedded UI.

`repo/Cargo.toml` lists these crates as workspace members with resolver 2. The
crate docs and dependencies preserve the layered flow: parser first, schema on
parser, database contracts on parser/schema, DuckDB on database/schema, then CLI
and API as executable surfaces.

## Decision

Keep the current six-crate workspace layout:
`swift-core`, `swift-schema`, `swift-db`, `swift-duckdb`, `swift-cli`, and
`swift-api`.

New functionality should land in the crate that owns its concern. Cross-cutting
changes should preserve the dependency direction instead of moving API or
DuckDB-specific behavior into lower-level crates.

## Consequences

- Parser and schema logic remain testable without API or DuckDB dependencies.
- Database-neutral contracts can evolve independently from the DuckDB adapter.
- CLI and API can share core behavior without becoming each other's dependency.
- Shared hardening gates can run across the workspace while still supporting
  focused per-crate tests.
- Adding another storage adapter or API surface should create a new leaf crate
  only when the existing ownership boundaries cannot represent it cleanly.

## Alternatives Considered

- Collapse everything into one crate. Rejected because it would couple parsing,
  schema authoring, storage adapters, CLI, and API concerns.
- Split every subsystem into smaller crates. Rejected because the current
  surface is still experimental and additional crates would add review and
  dependency overhead before clear ownership pressure exists.
- Put DuckDB behavior in `swift-db`. Rejected because `swift-db` is the
  database-neutral contract crate and should not depend on a concrete adapter.
- Make the API the orchestration root for all logic. Rejected because CLI and
  tests need the same lower-level behavior without HTTP dependencies.
