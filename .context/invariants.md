# Invariants

These rules should not be broken without an explicit architectural decision.

- Keep schemas as the primary control surface for message interpretation.
- Do not imply certified message coverage where only starter schemas exist.
- Document ingestion, validation, and rendering changes together when they share model implications.
- Preserve the `s3://bucket/key` object URI contract for job inputs, output
  prefixes, manifests, and object retrieval unless an ADR replaces
  [ADR-0002](../docs/decisions/ADR-0002-object-uri-s3-scheme.md).
- Keep Postgres as the system-of-record sink through the implemented SQLx path;
  do not reframe DuckDB as the production system of record without an ADR
  replacing [ADR-0003](../docs/decisions/ADR-0003-postgres-system-record-sqlx-sink.md).
- Keep API job submission behind bounded queue/backpressure semantics; queue
  saturation should fail explicitly instead of creating unbounded work
  ([ADR-0004](../docs/decisions/ADR-0004-bounded-in-process-job-queue.md)).
- Keep the local object-store adapter as the default implementation behind the
  stable `s3://` URI contract unless deployment-specific storage is explicitly
  configured ([ADR-0005](../docs/decisions/ADR-0005-local-disk-object-store-default.md)).
- Keep YAML as the editable schema authoring format and validated schema
  catalogs as the implementation boundary
  ([ADR-0006](../docs/decisions/ADR-0006-yaml-schema-authoring-format.md)).
- Keep render metadata beside parser metadata in schema files so parsing,
  normalization, and rendering changes remain reviewable together
  ([ADR-0007](../docs/decisions/ADR-0007-render-metadata-in-schema-files.md)).
- Keep shared bearer-token auth as the production-default API auth model until
  a stronger identity model is designed and recorded
  ([ADR-0008](../docs/decisions/ADR-0008-shared-bearer-token-auth.md)).
- Keep `tracing` spans/events and Prometheus metrics as the observability
  substrate ([ADR-0009](../docs/decisions/ADR-0009-tracing-prometheus-observability.md)).
- Preserve the six-crate workspace boundaries unless an ADR changes ownership
  or dependency direction
  ([ADR-0010](../docs/decisions/ADR-0010-six-crate-cargo-workspace.md)).
- Keep API render validation default-on for jobs unless callers explicitly opt
  out ([ADR-0011](../docs/decisions/ADR-0011-render-validation-default-on.md)).
- Keep user-facing API request structs strict about unknown fields so typoed or
  stale client payloads fail closed instead of being silently ignored.

## General agent rules

- Prefer small, reviewable diffs.
- Avoid architecture rewrites without an ADR.
- Update `.context/current-state.md` when meaningful project state changes.
- Add or update an ADR for major architectural decisions.
- Use `docs/tasks/current.md` as the active work queue.
- Run relevant tests or explain why they were not run.
- Never remove context files without explicit instruction.

## Open items

- Note: The earlier placeholder for deeper project-specific invariants is
  superseded by the concrete implementation constraints above.
