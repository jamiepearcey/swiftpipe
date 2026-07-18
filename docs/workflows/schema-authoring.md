# Schema Authoring Workflow

SwiftPipe schemas are YAML control-surface files under
`repo/examples/schemas/`. Each file defines reusable `field_types` and one or
more `messages`. The bundled schemas are starter schemas, not certified SRU
reproductions; exact network-rule coverage requires licensed SWIFT/ISO source
material.

## Add A Message Schema

Use an existing message file as the starting point for shape and naming. Keep
new schemas small enough to review:

- Add field parsers under `field_types`.
- Add a `messages` entry with `message`, `category`, `version`, and `coverage`.
- Model `sequences` with parent and repeat rules before adding fields.
- Add each field with `path`, `tag`, `name`, `type`, `entity`, and `column`.
- Add `required`, `min`, and `max` when cardinality is known.
- Add `options`, `option_types`, and `render` metadata for generic tags such as
  `98a`.

Use stable normalized names. `entity` groups fields into output tables; `column`
is the normalized field name in that entity. Avoid changing existing entity or
column names unless the downstream layout change is intentional.

## Validate The Corpus

Run the full schema validation pass:

```bash
cargo run -p swift-cli -- schema validate examples/schemas
```

Validate outbound render metadata:

```bash
cargo run -p swift-cli -- schema render-validate examples/schemas
```

Review coverage counters:

```bash
cargo run -p swift-cli -- schema coverage examples/schemas
```

Run schema reproduction tests:

```bash
cargo test -p swift-schema --test spec_reproduction
```

## Review Checklist

Before merging a new schema, confirm:

- `coverage.exact` is `false` unless the source is a licensed exact import.
- Coverage notes describe the source and any known gaps.
- Generic tags have unambiguous `render.option` and `render.qualifier` metadata.
- Required fields and non-repeatable sequences are represented explicitly.
- Sample fixtures and reproduction cases are added when source material is
  available.

## Common Failures

Ambiguous generic tags usually need `option_types` and `render.option`. Missing
render qualifiers usually need `render.qualifier` for fields whose parser type
captures a qualifier. Cardinality failures usually mean the sequence `parent`,
`parents`, or `repeat` rules do not match the sample message structure.
