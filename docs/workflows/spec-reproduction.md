# Spec Reproduction Workflow

Spec reproduction checks compare the checked-in MT sample corpus, local UHB
cache, YAML schemas, and schema tests. Use this workflow when adding a new
message schema, refreshing a sample, or auditing whether the starter schemas
still match the local source material.

The local UHB cache lives under `repo/examples/.uhb/`. It is a local artifact
for reproducibility only; do not replace licensed source obligations with the
starter schemas or copied snippets.

## Run The Full Check

From `repo/`, run the bundled reproduction gate:

```bash
bash ./scripts/check-spec-reproduction.sh
```

This runs formatting, the reproduction tests, UHB table parsing, schema
validation, render metadata validation, coverage reporting, and the full
`swift-schema` test suite.

## Replay Sequential Cases

Use the sequential runner when isolating a schema change to one or more message
types:

```bash
bash ./scripts/reproduce-specs-sequentially.sh MT540
```

The script accepts either `MT540` or a declared case name. With no arguments it
replays every declared reproduction case and then runs schema validation once at
the end.

To run only the validation phase through the same script:

```bash
RUN_VALIDATE_ONLY=1 bash ./scripts/reproduce-specs-sequentially.sh
```

To validate after each replayed case:

```bash
RUN_VALIDATE_AFTER_CASE=1 bash ./scripts/reproduce-specs-sequentially.sh MT540
```

## Inspect Coverage

List local reproduction coverage across cached UHB files, sample fixtures,
schemas, and declared tests:

```bash
bash ./scripts/spec-repro-coverage.sh
```

List the next reproduction actions inferred from the same inputs:

```bash
bash ./scripts/repro-worklist.sh
```

Use the worklist as an audit aid, not as an automatic source of truth. A message
is ready only when the source table is parseable, a sample exists, a schema
exists, and a reproduction case is declared.

## Add A Case

When source material is available, add a reproduction case in this order:

- Add or refresh `examples/.uhb/finmtNNN.md`.
- Add `examples/mtNNN_sample.fin` when the sample is available for the case.
- Add or update the relevant YAML schema under `examples/schemas/`.
- Add the case declaration in
  `crates/swift-schema/tests/spec_reproduction.rs`.
- Run the full check before relying on coverage output.

For schema-only changes without licensed exact source material, keep
`coverage.exact` false and document the source and known gaps in the schema
coverage notes.
