# Incident: Suspected Parquet Data Corruption

Use this checklist when a downstream Parquet export looks wrong, incomplete, or
inconsistent with the source FIN message. The goal is to preserve evidence,
reproduce on a disposable copy, and narrow the fault to source data, schema
matching, database materialization, export, or downstream consumption.

## Triage

- Stop automated retries that might overwrite the suspect export.
- Save the job ID, object URI or prefix, message type, output mode, schema
  version, and operator command or API request.
- Preserve the suspect Parquet files and the raw FIN input separately.
- Compare row counts across related entity tables before inspecting individual
  values.
- Check whether the issue is isolated to one message type, one entity table, or
  one output format.

## Reproduce On A Disposable Copy

From `repo/`, create an isolated copy of the local sample database and config:

```bash
rm -rf /tmp/swiftpipe-incident.duckdb /tmp/swiftpipe-incident.toml /tmp/swiftpipe-incident-parquet
cp examples/mt537.duckdb /tmp/swiftpipe-incident.duckdb
perl -0pe 's#database = "examples/mt537.duckdb"#database = "/tmp/swiftpipe-incident.duckdb"#g' \
  examples/mt537.swiftpipe.toml > /tmp/swiftpipe-incident.toml
```

Validate the schema catalog and render metadata before reprocessing:

```bash
cargo run -p swift-cli -- schema validate examples/schemas
cargo run -p swift-cli -- schema render-validate examples/schemas
```

Materialize and export from the copied database:

```bash
cargo run -p swift-cli -- run -c /tmp/swiftpipe-incident.toml --limit 1
cargo run -p swift-cli -- export -c /tmp/swiftpipe-incident.toml --format parquet --output /tmp/swiftpipe-incident-parquet
```

If the disposable export matches the suspect output, focus on schema or source
interpretation. If it differs, capture the exact config, CLI version, and
database copy before changing anything else.

## Narrow The Fault

Source-data issues usually show up in the raw FIN message and affect every
output format. Confirm the message type, sequence boundaries, repeated
sequences, and generic tag option letters before changing schema logic.

Schema-matching issues usually affect one message type or one entity family.
Look for missing required fields, unexpected repeated rows, ambiguous generic
tags, and coverage notes that say the schema is a starter schema rather than an
exact licensed reproduction.

Materialization issues usually affect multiple exports from the same DuckDB
database. Compare normalized table row counts and check whether a previous run
processed the message before the current schema was loaded.

Export issues usually affect Parquet but not CSV or rendered FIN output. Compare
the same copied database with CSV output before blaming schema matching.

Downstream issues usually reproduce only after another system reads the Parquet.
Preserve the SwiftPipe export and ask for the downstream reader version,
projection, filters, and schema evolution settings.

## Evidence To Attach

Attach the following to the incident record:

- Job ID or local command.
- Raw input URI or local fixture name.
- Schema directory and git state.
- Config file or API request body.
- Suspect Parquet files and reproduced Parquet files.
- Schema validation output.
- Row-count summary for affected entity tables.
- Any rendered FIN comparison if render validation is part of the investigation.

## Recovery

Do not delete suspect objects until the incident owner confirms evidence is
captured. Reprocess from preserved raw FIN inputs after the root cause is known.
If a schema fix is required, update the schema reproduction tests or coverage
notes before re-exporting downstream data.
