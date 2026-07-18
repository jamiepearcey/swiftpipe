# SwiftPipe

[![CI](https://github.com/jamiepearcey/swiftpipe/actions/workflows/ci.yml/badge.svg)](https://github.com/jamiepearcey/swiftpipe/actions/workflows/ci.yml)
[![Release](https://github.com/jamiepearcey/swiftpipe/actions/workflows/release.yml/badge.svg)](https://github.com/jamiepearcey/swiftpipe/actions/workflows/release.yml)

SwiftPipe is the experimental schema-driven SWIFT FIN ingestion engine.

Current vertical slice:

```text
DuckDB inbound table
  -> zero-copy FIN parser
  -> YAML schema matcher
  -> deterministic field-type parser
  -> inferred relational layout
  -> DuckDB writer
  -> schema-driven FIN renderer
```

Implemented local example schema corpus: 61 MT schemas under `examples/schemas`.

The bundled schemas are starter schemas, not certified SRU reproductions. Exact
network-rule reproduction needs licensed SWIFT/ISO source material imported into
the same YAML model.

To cache ISO20022 UHB pages locally (ignored output), use:

```bash
./scripts/fetch-uhb-specs.sh
```

To fetch specific messages only:

```bash
./scripts/fetch-uhb-specs.sh 537 540 541 542 543
```

Example commands:

```bash
cargo run -p swift-cli -- schema validate examples/schemas
cargo run -p swift-cli -- schema render-validate examples/schemas
cargo run -p swift-cli -- migrate --config examples/swiftpipe.toml
cargo run -p swift-cli -- run --config examples/swiftpipe.toml --limit 10000
cargo run -p swift-cli -- render --config examples/swiftpipe.toml --message-id msg-1 --validate --output out.fin
cargo run -p swift-cli -- render --config examples/swiftpipe.toml --all --validate --output-dir rendered-fin
```

Self-hosted API/harness:

```bash
RUST_LOG=swiftpipe_api=info,tower_http=warn cargo run -p swift-api -- \
  --listen 127.0.0.1:8080 \
  --schema-path examples/schemas \
  --object-root .swiftpipe-objects \
  --work-root .swiftpipe-work
```

Then open `http://127.0.0.1:8080/`.

Cross-origin browser access is disabled by default. Set
`SWIFTPIPE_CORS_ORIGINS` to a comma-separated allowlist, for example
`https://ops.example.com`, before serving the API to a browser-based UI from a
different origin.

Bearer auth is optional for local development when `SWIFTPIPE_AUTH_TOKEN` is
unset. Production deployments should pass `--auth-required` or set
`SWIFTPIPE_AUTH_REQUIRED=1`; startup then fails with a configuration error
unless `SWIFTPIPE_AUTH_TOKEN` contains at least one non-empty token.

Generated `exports.zip` artifacts are capped per job: 10 GiB total zip bytes and
100,000 entries by default. Tune these with `--zip-max-total-bytes` and
`--zip-max-entries` for constrained deployments.

Prefix jobs expand at most 10,000 objects by default and prepare matched inputs
with at most 4 worker threads per job. Tune these with `--max-prefix-fanout` and
`--max-prefix-parallelism`.

Docker:

```bash
docker build -t swiftpipe-api .
docker run --rm -p 8080:8080 \
  -v "$PWD/.swiftpipe-objects:/data/objects" \
  -v "$PWD/.swiftpipe-work:/data/work" \
  swiftpipe-api
```

The image bundles `examples/schemas` at `/app/schemas`. To run with a different
schema directory, mount it and override `--schema-path`:

```bash
docker run --rm -p 8080:8080 \
  -v "$PWD/my-schemas:/schemas:ro" \
  -v "$PWD/.swiftpipe-objects:/data/objects" \
  -v "$PWD/.swiftpipe-work:/data/work" \
  swiftpipe-api \
  --listen 0.0.0.0:8080 \
  --schema-path /schemas \
  --object-root /data/objects \
  --work-root /data/work
```

Optional system-of-record connection flags can be passed directly:

```bash
docker run --rm -p 8080:8080 \
  -v "$PWD/.swiftpipe-objects:/data/objects" \
  -v "$PWD/.swiftpipe-work:/data/work" \
  swiftpipe-api \
  --listen 0.0.0.0:8080 \
  --schema-path /app/schemas \
  --object-root /data/objects \
  --work-root /data/work \
  --system-of-record postgres \
  --postgres-connection "host=postgres port=5432 dbname=swiftpipe user=swiftpipe password=secret" \
  --system-record-schema swiftpipe
```

`--system-of-record sqlserver --sqlserver-connection "..."` is accepted as a
configuration shape, but the SQL Server adapter currently returns an explicit
unsupported error until ODBC/nanodbc support is implemented.

For throughput, the API keeps original FIN input durable in object storage and
does not duplicate raw message text into the DuckDB hydration export by default.
Pass `--persist-raw-text` if downstream consumers need `swift_raw_messages`
Parquet rows to include the full raw FIN text. Similarly, raw structural field
values are omitted from `swift_fields` by default; pass `--persist-raw-fields`
when those field-level raw values are required downstream.

API performance benchmark:

```bash
./scripts/benchmark-api.sh --target-bytes 500M --object-bytes 4M
```

That benchmark generates a local object-prefix corpus under `.swiftpipe-bench/`,
starts `swiftpipe-api`, submits a prefix job, and writes JSON results under
`.swiftpipe-bench/results/`. Each result records generation/process timings,
throughput, object locations, and replayable curl commands.

Current local release-mode prefix result, with raw FIN text kept in object
storage and raw payload columns omitted from DuckDB hydration exports, has
measured `500 MiB` in roughly `0.50-0.77s` (`~651-1006 MiB/s`) on this
machine. Use
`--persist-raw-text` to benchmark the slower mode that also duplicates full raw
FIN text into Parquet exports, and `--persist-raw-fields` to retain structural
raw field payloads.
The core parser microbenchmark is roughly `509-514 MiB/s` on a single sample
stream; the API prefix benchmark also includes object reads, schema
materialization, DuckDB writes, Parquet export, rendering, manifests, and zip
creation, while preparing independent objects in parallel.

To benchmark the DuckDB-bypass path that only renders FIN outputs:

```bash
./scripts/benchmark-api.sh --target-bytes 500M --object-bytes 4M --outputs rendered
```

Current local release-mode rendered-only result is roughly `500 MiB` in `0.15s`
(`~3424.7 MiB/s`) with `hydrate_write_ms: 0`.

To also exercise a single large HTTP upload:

```bash
./scripts/benchmark-api.sh --target-bytes 500M --object-bytes 4M --single-upload-bytes 500M
```

To generate the 500MB corpus without starting the API:

```bash
./scripts/benchmark-api.sh --target-bytes 500M --object-bytes 4M --single-upload-bytes 500M --generate-only
```

Use `--profile debug` only for smoke-testing the benchmark harness itself;
throughput numbers should come from the default release profile. Use
`--skip-build --api-binary /path/to/swiftpipe-api` when benchmarking a binary
built by CI or another machine.

The API currently provides a vendor-neutral local object-store implementation:
`s3://bucket/key` URIs are mapped under `--object-root/bucket/key`. This keeps
the job/artifact contract S3-compatible without requiring an AWS-specific SDK in
the hot path.

API endpoints:

```text
POST /api/upload?message_type=MT540&outputs=rendered
  Body: raw FIN message text. Stores input under s3://swiftpipe-inbox/uploads/,
  processes requested outputs, and returns manifest JSON. Omit outputs for the
  full artifact set.

POST /api/jobs
  Body: JSON with either input_uri or input_prefix. Prefix jobs snapshot matching
  objects before processing. Optional outputs values are rendered, parquet,
  errors, zip, manifest, or all.

GET /api/jobs/{job_id}/manifest
GET /api/object/{url-encoded-s3-uri}
```

Example prefix job:

```json
{
  "input_prefix": "s3://swiftpipe-inbox/daily/",
  "include_suffix": ".fin",
  "output_prefix": "s3://swiftpipe-outbox/jobs/my-job/",
  "message_type": "MT540",
  "render_validate": true,
  "outputs": ["rendered"]
}
```

Artifact layout:

```text
s3://swiftpipe-outbox/jobs/{job_id}/
  input_snapshot.json
  manifest.json
  errors.ndjson
  normalized/*.parquet
  rendered/*.fin
  exports.zip
```

DuckDB is only used for outputs that require relational hydration, currently
Parquet. Rendered-only jobs bypass DuckDB and write just manifest/status plus
`rendered/*.fin`.

System-of-record sync is optional and separate from DuckDB artifact hydration.
By default it is disabled:

```bash
target/release/swiftpipe-api \
  --system-of-record none
```

For local durability/testing, append job, audit, and artifact records to JSONL:

```bash
target/release/swiftpipe-api \
  --system-of-record file \
  --system-record-file .swiftpipe-work/system-record.jsonl
```

For Postgres, SwiftPipe uses a SQLx-backed sink and writes three tables:
`swiftpipe_jobs`, `swiftpipe_audit_events`, and `swiftpipe_artifacts`.

```bash
target/release/swiftpipe-api \
  --system-of-record postgres \
  --postgres-connection "host=localhost port=5432 dbname=swiftpipe user=swiftpipe" \
  --system-record-schema swiftpipe
```

SQL Server is reserved behind `--system-of-record sqlserver`; it currently
returns an explicit unsupported error until the ODBC/nanodbc adapter is wired.

Run the cached UHB spec parser checks (format table parser smoke test):

```bash
cargo test -p swift-schema --test uhb_spec_parser
```

Schema coverage:

```bash
cargo run -p swift-cli -- schema coverage examples/schemas
```

`schema render-validate` checks outbound-specific metadata such as generic tag
options and qualifiers. `schema coverage` reports render metadata counts plus
`ambiguous_render_options` and `missing_render_qualifiers` so outbound gaps are
visible before a message is rendered.

All-five DuckDB demo:

```bash
rm -f examples/all5.duckdb examples/all5.duckdb.wal
duckdb examples/all5.duckdb "CREATE TABLE inbound_messages (id TEXT NOT NULL, message_type TEXT NOT NULL, body TEXT NOT NULL, processed BOOLEAN NOT NULL DEFAULT false); INSERT INTO inbound_messages SELECT 'msg-537', 'MT537', content, false FROM read_text('examples/mt537_sample.fin'); INSERT INTO inbound_messages SELECT 'msg-540', 'MT540', content, false FROM read_text('examples/mt540_sample.fin'); INSERT INTO inbound_messages SELECT 'msg-541', 'MT541', content, false FROM read_text('examples/mt541_sample.fin'); INSERT INTO inbound_messages SELECT 'msg-542', 'MT542', content, false FROM read_text('examples/mt542_sample.fin'); INSERT INTO inbound_messages SELECT 'msg-543', 'MT543', content, false FROM read_text('examples/mt543_sample.fin');"
cargo run -p swift-cli -- migrate --config examples/all5.swiftpipe.toml
cargo run -p swift-cli -- run --config examples/all5.swiftpipe.toml --limit 100
cargo run -p swift-cli -- render --config examples/all5.swiftpipe.toml --message-id msg-540 --validate --output /tmp/mt540.fin
cargo run -p swift-cli -- render --config examples/all5.swiftpipe.toml --all --validate --output-dir /tmp/all5-fin
cargo run -p swift-cli -- export --config examples/all5.swiftpipe.toml --output examples/all5-output --format csv --format parquet
cargo run -p swift-cli -- export --config examples/all5.swiftpipe.toml --output examples/all5-parquet --format parquet --parquet-row-group-size 100000
```

Expected all-five validation counts:

```text
messages: 5
field rows: 119
normalized rows: 35
parse errors: 0
exported files: 64
```

The first DuckDB inbound table shape expected by the example config is:

```sql
CREATE TABLE inbound_messages (
  id TEXT NOT NULL,
  message_type TEXT NOT NULL,
  body TEXT NOT NULL,
  processed BOOLEAN NOT NULL DEFAULT false
);
```

Modeling schema shape:

- `field_types` define reusable deterministic parsers as literal/capture/rest
  steps.
- `messages` define SWIFT message type, sequence graph, field cardinality,
  option-letter tags such as `98a`, qualifier filters, and normalized
  entity/column mapping.
- Optional `render` metadata defines outbound tag options and qualifiers where
  the parser schema is ambiguous. Inbound materialization also writes
  `__render_*` payload columns so outbound rendering can preserve SWIFT prefixes
  such as `UNIT/` and currency codes while still using edited normalized numeric
  values.
- The database layout is inferred from the normalized entity/column mappings,
  with raw messages, parsed field rows, parse errors, and normalized tables.

Validation:

```bash
cargo fmt --check
cargo test
```

Use the pre-commit-style validation script:

```bash
./scripts/check-spec-reproduction.sh
```

Run reproduction checks one-by-one per spec test:

```bash
./scripts/reproduce-specs-sequentially.sh
./scripts/reproduce-specs-sequentially.sh 540 541 542 543 537
```

The runner auto-discovers tests from `crates/swift-schema/tests/spec_reproduction.rs`, so adding a new
spec test automatically includes it in the default sequence.

To see the next actionable repro targets from the cached UHB specs:

```bash
./scripts/repro-worklist.sh
```

That command prints each cached spec’s parser status and whether it is blocked by
missing sample/schema/test assets.
