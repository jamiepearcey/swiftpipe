# ADR-0007: Keep Render Metadata In Schema Files

## Status

Accepted

## Context

SwiftPipe schemas describe both inbound parsing and outbound rendering. Generic
SWIFT tags, qualifiers, sequence paths, field cardinality, normalized columns,
and render reconstruction rules all refer to the same field-level facts. If
render metadata lived in a separate file, authors would have to keep two
control surfaces synchronized for every field rule.

The implemented schema model keeps render metadata on field rules:

- `repo/crates/swift-schema/src/lib.rs` defines `FieldRuleSchema::render` and
  `FieldRenderSchema`.
- `SchemaCatalog::validate_rendering` validates base schema rules and render
  metadata together.
- `validate_render_metadata` requires render options and qualifiers where
  generic tags or parser captures would otherwise be ambiguous.
- `render_payload_column` derives render payload columns from the same field
  rule that defines the normalized output column.
- `repo/crates/swift-schema/tests/render_metadata.rs` checks every local sample
  schema for unambiguous render metadata.
- `repo/crates/swift-schema/tests/goldens.rs` and
  `repo/crates/swift-schema/tests/roundtrip.rs` validate normalized output and
  parse-render-reparse behavior against the same YAML schema corpus.

## Decision

Keep outbound render metadata in the same YAML schema file and field rule as
the parser metadata it disambiguates.

Render metadata should remain optional only when the parser metadata is already
unambiguous. When rendering needs an option, qualifier, or format hint, that
hint belongs beside the field rule that defines the tag, path, type, entity, and
column.

## Consequences

- Schema authors review inbound and outbound behavior for a field in one place.
- Validation can catch parser/render inconsistencies before runtime rendering.
- Roundtrip and golden tests exercise one schema catalog instead of reconciling
  separate parser and renderer catalogs.
- Schema files are denser because parsing and rendering concerns are coupled in
  the same field entries.
- A future split would need a clear stability boundary, migration tooling, and
  tests proving parser and render metadata cannot drift.

## Alternatives Considered

- Store render metadata in a separate renderer YAML file. Rejected because it
  would duplicate field identities and make drift likely.
- Infer all render metadata from parser metadata. Rejected because generic tags
  and qualifier-bearing field types often need explicit disambiguation.
- Store render metadata in generated code. Rejected because render behavior
  would become harder to review alongside schema changes.
- Defer render metadata until a separate outbound product exists. Rejected
  because current render validation and roundtrip coverage already depend on
  field-level render hints.
