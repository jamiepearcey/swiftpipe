# Benchmarking Workflow

Run Criterion benches from `repo/`:

```sh
cargo bench -p swift-schema --bench infer_database_layout
```

Compile all benchmark targets without running measurements:

```sh
cargo bench --workspace --no-run
```

Prefix-job API benchmarks should record `--max-prefix-parallelism`; the default
is 4 worker threads per prefix job, further bounded by available CPUs and matched
input count.

CLI Parquet exports can set `--parquet-row-group-size`. Start with
`50,000-250,000` rows for general local export profiling: smaller values can
reduce peak memory during constrained runs, while larger values can improve scan
efficiency for wide or high-volume normalized tables. Record the selected value
with any Parquet export measurement.

Record the command, machine class, date, and Criterion median/range in the task or commit summary when a perf task changes code.

## Interpreting Results

Treat Criterion's median estimate as the primary comparison point and the
reported range as the noise band. A run is usually healthy when the new median
stays inside the previous range, or when a small slowdown has a clear reason
such as stricter validation, additional output, or a larger fixture.

Flag a result for investigation when:

- The median moves by more than 5% on parser, schema, or materialization
  benches without an intentional behavior change.
- Allocation audit benches increase allocation counts on hot parser or renderer
  paths.
- Throughput drops while fixture size and output selection are unchanged.
- A benchmark changes fixture shape, message count, row count, parallelism, or
  row-group size without recording the new values.

Good numbers are stable and explainable. For this repo that means the command,
fixture, machine class, date, median range, throughput, and important tunables
are captured together. A faster number from a changed fixture is not comparable
until the fixture change is called out.

## Common Regressions

Parser regressions usually come from extra string allocation, repeated scans of
field text, or deeper sequence bookkeeping in `swift-core`. Use the parser
allocation bench to separate throughput noise from allocation growth.

Schema regressions usually come from reloading YAML, rebuilding catalog-derived
indexes, or walking all messages for work that should be message-local. Compare
`swift-schema` layout inference and renderer allocation results before changing
shared schema traversal code.

DuckDB regressions usually come from smaller write chunks, extra transaction
boundaries, or export option changes. Always record the write batch size and
Parquet row-group size when those knobs are part of the measurement.

API regressions usually come from serial prefix processing, zip buffering
changes, extra object-store round trips, or schema reloads on the job path.
Record output mode and `--max-prefix-parallelism` whenever prefix-job throughput
is measured.

## Bisecting A Regression

Start with the smallest benchmark that isolates the affected layer. Parser
changes should begin with `swift-core`; schema layout or render changes should
begin with `swift-schema`; storage and export changes should begin with
`swift-duckdb`; job orchestration changes should begin with `swift-api`.

Keep each bisect run comparable:

- Use the same target directory unless a clean rebuild is the question.
- Keep fixture files and generated corpus sizes unchanged.
- Record environment variables that affect parallelism or output format.
- Prefer one benchmark target over the workspace when narrowing a regression.
- Re-run the suspected good and bad commits once before attributing the change.

When the regression crosses crate boundaries, test from the leaf dependency
upward. For example, parser first, then schema matching, then database
materialization, then API job processing. That order prevents an API-level
throughput drop from hiding a lower-level parser or schema allocation change.

## Local Measurements

### README-Pinned Historical API Baselines

- Command: `./scripts/benchmark-api.sh --target-bytes 500M --object-bytes 4M`
- Fixture: generated 500 MiB local object-prefix corpus with 4 MiB objects.
- Result: `500 MiB` in roughly `0.50-0.77s`.
- Throughput: `~651-1006 MiB/s`.
- Source: pinned from `repo/README.md` as the historical local release-mode prefix baseline.
- Command: `./scripts/benchmark-api.sh --target-bytes 500M --object-bytes 4M --outputs rendered`
- Fixture: generated 500 MiB local object-prefix corpus with 4 MiB objects, rendered-only outputs.
- Result: roughly `500 MiB` in `0.15s`.
- Throughput: `~3424.7 MiB/s`.
- Source: pinned from `repo/README.md` as the historical local release-mode rendered-only baseline.

### 2026-06-02 - `swift-core` Parser

- Command: `cargo bench -p swift-core --bench parse_message`
- Fixture: representative SEMT structural sample and generated 500 KiB SWIFT FIN message.
- Bench: `swift_core_parse_message/semt_structural`
- Result: `time: [659.28 ns 669.21 ns 679.30 ns]`
- Throughput: `thrpt: [548.92 MiB/s 557.21 MiB/s 565.60 MiB/s]`
- Bench: `swift_core_parse_message/parse_500kb`
- Result: `time: [247.02 µs 248.77 µs 250.80 µs]`
- Throughput: `thrpt: [1.9015 GiB/s 1.9169 GiB/s 1.9305 GiB/s]`
- Note: Gnuplot was not installed, so Criterion used the plotters backend. Compared with the pre-cleanup measurement in this task (`696.91 ns` SEMT mean, `249.39 µs` 500 KiB mean), the final parser bench improved the SEMT mean time and kept the large fixture within noise.

### 2026-06-02 - `swift-core` Parser Allocation Audit

- Command: `cargo bench -p swift-core --bench parse_allocations`
- Fixture: representative SEMT structural sample and generated 500 KiB SWIFT FIN message.
- Bench: `swift_core_parse_allocations/semt_structural`
- Result: `time: [655.67 ns 677.89 ns 713.47 ns]`
- Allocation count: 2 allocations per parse.
- Bench: `swift_core_parse_allocations/parse_500kb`
- Result: `time: [349.11 µs 437.18 µs 532.94 µs]`
- Allocation count: 2 allocations per parse.
- Note: Uses a portable counting global allocator as a local substitute for cargo-instruments/dhat-rs; use this bench for allocation counts, not parser throughput.

### 2026-06-02 - `swift-schema` Layout Inference

- Command: `cargo bench -p swift-schema --bench infer_database_layout`
- Fixture: merged `repo/examples/schemas` corpus, 61 message schemas.
- Bench: `swift_schema_infer_database_layout/full_example_schema_corpus`
- Result: `time: [436.98 µs 439.61 µs 442.87 µs]`
- Throughput: `thrpt: [137.74 Kelem/s 138.76 Kelem/s 139.60 Kelem/s]`
- Note: Gnuplot was not installed, so Criterion used the plotters backend.

### 2026-06-02 - `swift-schema` Renderer Allocation Audit

- Command: `cargo bench -p swift-schema --bench render_allocations`
- Fixture: synthetic MT540 render request with three normalized rows across `GENL` and `LINK`.
- Bench: `swift_schema_render_allocations/mt540_three_fields`
- Result: `time: [3.7055 µs 3.8277 µs 3.9981 µs]`
- Allocation count: 120 allocations per render.
- Note: Uses a portable counting global allocator as a local substitute for cargo-instruments/dhat-rs.

### 2026-06-02 - `swift-db` MT540 Materialization

- Command: `cargo bench -p swift-db --bench materialize_message`
- Fixture: `repo/examples/mt540_sample.fin` with `repo/examples/schemas/mt540.yaml`.
- Bench: `swift_db_materialize_message/mt540_sample`
- Result: `time: [33.626 µs 33.963 µs 34.416 µs]`
- Throughput: `thrpt: [610.19 Kelem/s 618.32 Kelem/s 624.52 Kelem/s]`
- Note: Gnuplot was not installed, so Criterion used the plotters backend.

### 2026-06-02 - `swift-duckdb` 10k Row Batch Write

- Command: `CARGO_TARGET_DIR=/private/tmp/swiftpipe-duckdb-bench-target cargo bench -p swift-duckdb --bench write_batch`
- Fixture: synthetic `ParsedOutputBatch` with 10,000 normalized rows for one settlement table.
- Bench: `swift_duckdb_write_batch/normalized_10k_rows`
- Result: `time: [777.65 ms 783.08 ms 790.73 ms]`
- Throughput: `thrpt: [12.647 Kelem/s 12.770 Kelem/s 12.859 Kelem/s]`
- Write batch size: default `1,000` rows per chunk inside the existing transaction.
- Note: Gnuplot was not installed, so Criterion used the plotters backend.

### 2026-06-02 - `swift-cli` Parquet Row-Group Export Flag

- Command: `cargo test -p swift-duckdb tests::builds_copy_options_for_parquet_row_group_size -- --exact`
- Fixture: unit coverage for DuckDB `COPY` option generation.
- Result: passed; CLI Parquet exports can pass a non-zero
  `--parquet-row-group-size`, which renders as DuckDB `ROW_GROUP_SIZE` for
  Parquet only and leaves CSV exports unchanged.
- Recommended tuning range: start with `50,000-250,000` rows, then benchmark the
  target schema/data shape and record the chosen value.

### 2026-06-02 - `swift-api` Zip Export

- Command: `cargo bench -p swift-api --bench zip_export`
- Fixture: synthetic local object-store prefix with 100 text artifacts, 16 KiB each.
- Bench: `swift_api_zip_export/100_text_artifacts`
- Result: `time: [6.4423 ms 6.6186 ms 6.8384 ms]`
- Throughput: `thrpt: [228.49 MiB/s 236.08 MiB/s 242.54 MiB/s]`
- Note: Gnuplot was not installed, so Criterion used the plotters backend.
  This run streams `ZipWriter` output through a `BufWriter<File>` temp file
  before atomic rename; Criterion reported no statistically significant
  performance change from the prior baseline.

### 2026-06-02 - `swift-api` Schema Catalog Startup Cache

- Command: `cargo test -p swift-api upload_uses_cached_schema_catalog_after_schema_directory_is_removed -- --exact`
- Fixture: sync upload router built from a temporary copy of the example schema
  corpus, with that schema directory deleted before the upload request.
- Result: passed; API jobs use the `SchemaCatalog` cached in `AppState` at
  router/startup construction instead of reloading schema YAML for each request.
