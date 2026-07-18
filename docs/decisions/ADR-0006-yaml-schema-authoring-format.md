# ADR-0006: Use YAML As The Schema Authoring Format

## Status

Accepted

## Context

SwiftPipe schemas are the declarative control surface for message matching,
sequence structure, normalized output layout, and render metadata. They need to
be readable enough for review against SWIFT examples while structured enough for
validation and future compilation into faster execution plans.

The implemented format is YAML:

- `repo/crates/swift-schema/src/lib.rs` exposes
  `SchemaCatalog::from_yaml_str` and parses with `serde_yaml`.
- Schema structs use `serde(deny_unknown_fields)` so misspelled authoring keys
  fail validation instead of being silently ignored.
- `repo/crates/swift-cli/src/main.rs` loads `.yaml` and `.yml` files from schema
  paths and merges catalogs for CLI commands.
- `repo/examples/schemas/*.yaml` contains the bundled starter schema corpus.
- `repo/crates/swift-schema/tests/goldens.rs`,
  `repo/crates/swift-schema/tests/roundtrip.rs`, and
  `repo/crates/swift-schema/tests/spec_reproduction.rs` load those YAML files
  for regression coverage.
- `docs/workflows/schema-authoring.md` documents YAML schema authoring and the
  validation commands.

## Decision

Use YAML as the external schema authoring format for SwiftPipe.

YAML remains the human-authored source of truth. Internal code may derive,
cache, or compile validated catalogs for performance, but those generated
representations do not replace the YAML authoring contract.

## Consequences

- Schema changes are reviewable as text and can group repeated message concepts
  naturally with mappings and lists.
- Validation can reject unknown fields, duplicate definitions, missing field
  types, cardinality issues, and ambiguous render metadata.
- YAML-specific risks such as aliases or surprising scalar parsing must be
  covered by parser tests and conservative validation.
- A future generated or compiled representation must be derived from validated
  YAML rather than becoming a competing manual authoring surface.

## Alternatives Considered

- JSON. Rejected because it is more verbose for nested schema authoring and
  lacks comments in the standard format, which makes field-by-field review
  harder.
- TOML. Rejected because deeply nested repeated structures such as message
  fields, sequence maps, and render metadata become awkward and less readable.
- A custom DSL. Rejected because it would require a bespoke parser, formatter,
  editor support, and migration path before the schema model itself is stable.
- Rust code or generated code as the authoring source. Rejected because it would
  make schema changes harder for non-Rust reviewers and couple authoring to
  crate releases.
