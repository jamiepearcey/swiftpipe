# Current State

## Current known implementation state

- Container structure exists with repo, notes, and research.
- Inner repo ships an experimental but hardened SWIFT FIN ingestion vertical
  slice with parser, schema, normalization, DuckDB export, API, CLI, UI, CI,
  deployment docs, runbooks, and ADRs.
- Project is clearly marked experimental rather than production-hardened.
- Schemas remain the primary control surface for parsing, validation,
  normalization, and rendering; starter schemas must not be described as
  certified coverage.
- The API accepts `s3://bucket/key` object URIs, uses a bounded in-process job
  queue, defaults render validation on, persists artifacts through a local
  object-store adapter, and can sync job/artifact events to Postgres through a
  SQLx system-of-record sink.
- The repository now includes CI gates for Rust fmt, clippy, tests, rustdoc,
  coverage, cargo-deny, cargo-audit, cargo-machete, cargo-sort, shellcheck,
  actionlint, Docker lint/build/scan, release planning, and UI install/audit/
  lint/test/build/artifact upload.
- Current implementation decisions are indexed in [docs/index.md](../docs/index.md)
  and ADR-0002 through ADR-0011.
- [.context/invariants.md](invariants.md) now records concrete ADR-backed
  invariants for object URIs, Postgres, bounded queues, schema authoring,
  render metadata, auth, observability, workspace boundaries, render
  validation, and strict API request decoding.
- [docs/tasks/current.md](../docs/tasks/current.md) now points at
  `cgpt-queue/` as the live autonomous hardening queue.
- [docs/tasks/backlog.md](../docs/tasks/backlog.md) now separates unresolved
  future work from completed schema, ADR, and experimental-status documentation.
- [docs/architecture/system-overview.md](../docs/architecture/system-overview.md)
  now documents subsystem flow and ownership boundaries across crates, UI,
  deployment, and ADR-backed constraints.
- [docs/index.md](../docs/index.md) now includes a dedicated workflows and
  runbooks landing section.
- [docs/tasks/cgpt-queue-summary.md](../docs/tasks/cgpt-queue-summary.md)
  records completion of tasks 309-355 by section, notable decisions,
  cannot-proceed status, and bugs/risks found.

## Recently completed work

- Added **anchored sequences** (ADR-0013): `swift-core` and `swift-schema`
  now support message types whose repeating field group has no `:16R:`/
  `:16S:` wrapper (a schema declares an `anchor_tag`/`member_tags` sequence).
  Added a real, spec-correct `examples/schemas/mt940.yaml` — MT940 (SWIFT
  Category 9 Customer Statement) now flows through the exact same
  parse/materialize/render pipeline as every other MT type, with zero
  special-casing in `swift-api`. Fixed `render_block4` (it unconditionally
  emitted `:16R:`/`:16S:` for every sequence — anchored ones must not render a
  wrapper). Added `examples/.uhb/finmt940.md` (manually transcribed from the
  authoritative SWIFT Category 9 spec — iso20022.org's UHB catalogue 404s for
  MT940; it only covers the later ISO 15022 generic-field message family) and
  registered MT940 in the golden/spec-reproduction test suites (62 schemas
  now, was 61). Fixed a real console bug found along the way: `Parser.tsx`
  stripped the `MT` prefix before sending `message_type` to `swift-api`,
  which never matched the schema catalog's keys — the "Validate on server"
  button silently mismatched for every message type, not just MT940. Full
  workspace `cargo test` green (52 test groups, 0 failures) including the
  swift-duckdb full-corpus round-trip (all 62 schemas) and the UHB
  spec-reproduction suite.
- Added the **CSDR cash-penalty framework** (ADR-0012): a new pure crate
  `ingest-penalty` computes expected settlement-fail penalties (SEFP =
  rate_bps/10_000 × reference_amount, starter rate table by instrument type)
  from failing/pending MT537 events, imports the CSD monthly statement
  (`--penalty-statement CSV`), and reconciles computed vs reported → breaks.
  `SecurityEvent` gained `amount`/`currency` (the penalty base). The `ingest
  serve` read-model gained `GET /csdr/snapshot`; `ingest store` writes
  `penalty_accruals.parquet` + `penalty_statements.parquet`. Both the swiftpipe
  console and the quant/pricing UI surface a CSDR penalty-recon section over
  `/csdr`. Sample data: `examples/mt537_{gilt,bond}_sample.fin` +
  `examples/mt537_penalty_statement.csv`. Starter/non-certified (see ADR-0012).
- Added MT537 (Statement of Pending Transactions) support end-to-end: the
  normalized `SecurityEvent` now carries an optional `status`, `swift-normalize`
  captures the per-transaction status code (`:25D::IPRC//PEND`) and the MT537
  safekeeping account, `ingest-parquet` persists `status` on the data plane,
  `ingest-tabular` reads an optional `status` column (cross-source), and a real
  MT537 fixture test asserts `status = "PEND"`. Fixed the `mt537_qualified_status`
  schema type (`/` → `//`) so `:25D:`/`:24B:` codes capture cleanly; MT537 golden
  refreshed.
- Moved into finance container structure.
- Added project-level overview documentation.
- Hardened `swift-core` with crate-level deny attributes for warnings,
  Rust 2018 idioms, unsafe code, and missing debug implementations.
- Hardened `swift-schema` with the same crate-level deny attributes.
- Hardened `swift-db` with the same crate-level deny attributes.
- Hardened `swift-duckdb` with the same crate-level deny attributes and a
  stable manual `Debug` implementation for the DuckDB store configuration.
- Hardened `swift-cli` with the same crate-level deny attributes.
- Hardened `swift-api` with the same crate-level deny attributes.
- Enabled curated `clippy::pedantic` linting for `swift-core`.
- Enabled curated `clippy::pedantic` linting for `swift-schema`.
- Enabled curated `clippy::pedantic` linting for `swift-db`.
- Enabled curated `clippy::pedantic` linting for `swift-duckdb`.
- Enabled curated `clippy::pedantic` linting for `swift-cli`.
- Enabled curated `clippy::pedantic` linting for `swift-api`.
- Pinned `swift-core` MSRV metadata to Rust 1.82 to match the Docker base
  toolchain.
- Pinned `swift-schema` MSRV metadata to Rust 1.82.
- Pinned `swift-db` MSRV metadata to Rust 1.82.
- Pinned `swift-duckdb` MSRV metadata to Rust 1.82.
- Pinned `swift-cli` MSRV metadata to Rust 1.82.
- Pinned `swift-api` MSRV metadata to Rust 1.82.
- Added a CI MSRV job that checks the workspace with Rust 1.82 and a
  dedicated Cargo cache key.
- Restored the workspace `cargo fmt --check` and
  `cargo clippy --workspace --all-targets -- -D warnings` gates while
  preserving the existing API job-processing behavior.
- Removed non-test panic-prone startup/shutdown `expect` calls from
  `swift-api`; queue workers now receive an intentionally queue-less state
  and signal handler setup failures are reported with contextual errors.
- Removed non-test panic-prone `expect` calls from `swift-api` job processing;
  DuckDB hydration invariant failures and input worker thread panics now flow
  through contextual job errors instead of crashing the process.
- Removed non-test panic-prone `expect` calls from `swift-api` job store lock
  access by recovering poisoned `RwLock` guards instead of crashing callers.
- Removed panic-prone JSON serialization `expect` calls from `swift-api`
  routes; handler serialization failures now return internal API errors, while
  static response builders retain documented construction invariants.
- Removed the production normalized-row insert `expect` from `swift-duckdb`;
  missing grouped columns now return a contextual `DuckDbAdapterError` instead
  of panicking.
- Removed production `expect` calls from `swift-schema` rendering and sequence
  validation paths; string rendering uses an infallible helper and missing
  filtered render columns return existing render errors.
- Strengthened `swift-core` unsafe policy from deny to
  `#![forbid(unsafe_code)]`.
- Strengthened `swift-schema` unsafe policy from deny to
  `#![forbid(unsafe_code)]`.
- Strengthened `swift-db` unsafe policy from deny to
  `#![forbid(unsafe_code)]`.
- Strengthened `swift-duckdb` unsafe policy from deny to
  `#![forbid(unsafe_code)]`.
- Strengthened `swift-cli` unsafe policy from deny to
  `#![forbid(unsafe_code)]`.
- Strengthened `swift-api` unsafe policy from deny to
  `#![forbid(unsafe_code)]`.
- Added `repo/rustfmt.toml` with explicit edition, width, import/module
  ordering, and shorthand formatting rules.
- Added `repo/deny.toml` and a CI `cargo deny check` job for advisory,
  license, duplicate-version, and source policy enforcement.
- Upgraded `swift-api` to `sqlx` 0.8.x to clear cargo-deny advisory
  failures from the older SQLx/Rustls dependency chain.
- Marked workspace crates `publish = false` so cargo-deny treats them as
  private internal crates rather than requiring an invented publication
  license.
- Added a CI `cargo machete` job to fail unused workspace dependencies before
  they inflate compile time or supply-chain surface.
- Added a warn-only nightly CI `cargo udeps --workspace --all-targets` job to
  catch dependency drift that machete misses without making nightly instability
  block the main build.
- Added a CI `cargo sort --workspace --check` step and sorted workspace
  manifests so dependency ordering drift is caught early.
- Added an MT321 parse-render-reparse structural roundtrip regression test in
  `swift-schema` and tightened the MT321 `trade_date` qualifier so settlement
  dates no longer render as extra trade dates.
- Extended the structural roundtrip regression coverage to MT370 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT380 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT381 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT500 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT501 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT502 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT503 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT504 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT505 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT506 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT507 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT508 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT509 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT510 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT513 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT514 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT515 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT516 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT517 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT518 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT519 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT524 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT526 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT527 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT530 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT535 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT536 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT537 and sorted
  structural field tuples before comparison so canonical render ordering remains
  a set-equivalence roundtrip check.
- Extended the structural roundtrip regression coverage to MT538 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT540 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT541 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT542 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT543 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT544 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT545 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT546 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT547 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT548 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT549 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT558 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT564 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT565 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT566 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT567 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT568 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT569 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT575 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT576 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT578 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT581 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT586 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT590 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT591 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT592 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT595 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT596 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT598 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT599 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT670 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Extended the structural roundtrip regression coverage to MT671 and tightened
  its `trade_date` qualifier for the same settlement-date overmatch.
- Added an MT321 normalized-output golden snapshot test in `swift-schema` with
  sorted NDJSON records and an `UPDATE_GOLDENS=1` refresh path.
- Extended the normalized-output golden snapshot coverage to MT370.
- Extended the normalized-output golden snapshot coverage to MT380.
- Extended the normalized-output golden snapshot coverage to MT381.
- Extended the normalized-output golden snapshot coverage to MT500.
- Extended the normalized-output golden snapshot coverage to MT501.
- Extended the normalized-output golden snapshot coverage to MT502.
- Extended the normalized-output golden snapshot coverage to MT503.
- Extended the normalized-output golden snapshot coverage to MT504.
- Extended the normalized-output golden snapshot coverage to MT505.
- Extended the normalized-output golden snapshot coverage to MT506.
- Extended the normalized-output golden snapshot coverage to MT507.
- Extended the normalized-output golden snapshot coverage to MT508.
- Extended the normalized-output golden snapshot coverage to MT509.
- Extended the normalized-output golden snapshot coverage to MT510.
- Extended the normalized-output golden snapshot coverage to MT513.
- Extended the normalized-output golden snapshot coverage to MT514.
- Extended the normalized-output golden snapshot coverage to MT515.
- Extended the normalized-output golden snapshot coverage to MT516.
- Extended the normalized-output golden snapshot coverage to MT517.
- Extended the normalized-output golden snapshot coverage to MT518.
- Extended the normalized-output golden snapshot coverage to MT519.
- Extended the normalized-output golden snapshot coverage to MT524.
- Extended the normalized-output golden snapshot coverage to MT526.
- Extended the normalized-output golden snapshot coverage to MT527.
- Extended the normalized-output golden snapshot coverage to MT530.
- Extended the normalized-output golden snapshot coverage to MT535.
- Extended the normalized-output golden snapshot coverage to MT536.
- Extended the normalized-output golden snapshot coverage to MT537.
- Extended the normalized-output golden snapshot coverage to MT538.
- Extended the normalized-output golden snapshot coverage to MT540.
- Extended the normalized-output golden snapshot coverage to MT541.
- Extended the normalized-output golden snapshot coverage to MT542.
- Extended the normalized-output golden snapshot coverage to MT543.
- Extended the normalized-output golden snapshot coverage to MT544.
- Extended the normalized-output golden snapshot coverage to MT545.
- Extended the normalized-output golden snapshot coverage to MT546.
- Extended the normalized-output golden snapshot coverage to MT547.
- Extended the normalized-output golden snapshot coverage to MT548.
- Extended the normalized-output golden snapshot coverage to MT549.
- Extended the normalized-output golden snapshot coverage to MT558.
- Extended the normalized-output golden snapshot coverage to MT564.
- Extended the normalized-output golden snapshot coverage to MT565.
- Extended the normalized-output golden snapshot coverage to MT566.
- Extended the normalized-output golden snapshot coverage to MT567.
- Extended the normalized-output golden snapshot coverage to MT568.
- Extended the normalized-output golden snapshot coverage to MT569.
- Extended the normalized-output golden snapshot coverage to MT575.
- Extended the normalized-output golden snapshot coverage to MT576.
- Extended the normalized-output golden snapshot coverage to MT578.
- Extended the normalized-output golden snapshot coverage to MT581.
- Extended the normalized-output golden snapshot coverage to MT586.
- Extended the normalized-output golden snapshot coverage to MT590.
- Extended the normalized-output golden snapshot coverage to MT591.
- Extended the normalized-output golden snapshot coverage to MT592.
- Extended the normalized-output golden snapshot coverage to MT595.
- Extended the normalized-output golden snapshot coverage to MT596.
- Extended the normalized-output golden snapshot coverage to MT598.
- Extended the normalized-output golden snapshot coverage to MT599.
- Extended the normalized-output golden snapshot coverage to MT670.
- Extended the normalized-output golden snapshot coverage to MT671.
- Added a focused render metadata coverage test harness starting with MT321 sample-matched fields.
- Extended focused render metadata coverage to MT370 sample-matched fields.
- Extended focused render metadata coverage to MT380 sample-matched fields.
- Extended focused render metadata coverage to MT381 sample-matched fields.
- Extended focused render metadata coverage to MT500 sample-matched fields.
- Extended focused render metadata coverage to MT501 sample-matched fields.
- Extended focused render metadata coverage to MT502 sample-matched fields.
- Extended focused render metadata coverage to MT503 sample-matched fields.
- Extended focused render metadata coverage to MT504 sample-matched fields.
- Extended focused render metadata coverage to MT505 sample-matched fields.
- Extended focused render metadata coverage to MT506 sample-matched fields.
- Extended focused render metadata coverage to MT507 sample-matched fields.
- Extended focused render metadata coverage to MT508 sample-matched fields.
- Extended focused render metadata coverage to MT509 sample-matched fields.
- Extended focused render metadata coverage to MT510 sample-matched fields.
- Extended focused render metadata coverage to MT513 sample-matched fields.
- Extended focused render metadata coverage to MT514 sample-matched fields.
- Extended focused render metadata coverage to MT515 sample-matched fields.
- Extended focused render metadata coverage to MT516 sample-matched fields.
- Extended focused render metadata coverage to MT517 sample-matched fields.
- Extended focused render metadata coverage to MT518 sample-matched fields.
- Extended focused render metadata coverage to MT519 sample-matched fields.
- Extended focused render metadata coverage to MT524 sample-matched fields.
- Extended focused render metadata coverage to MT526 sample-matched fields.
- Extended focused render metadata coverage to MT527 sample-matched fields.
- Extended focused render metadata coverage to MT530 sample-matched fields.
- Extended focused render metadata coverage to MT535 sample-matched fields.
- Extended focused render metadata coverage to MT536 sample-matched fields.
- Extended focused render metadata coverage to MT537 sample-matched fields.
- Extended focused render metadata coverage to MT538 sample-matched fields.
- Extended focused render metadata coverage to MT540 sample-matched fields.
- Extended focused render metadata coverage to MT541 sample-matched fields.
- Extended focused render metadata coverage to MT542 sample-matched fields.
- Extended focused render metadata coverage to MT543 sample-matched fields.
- Extended focused render metadata coverage to MT544 sample-matched fields.
- Extended focused render metadata coverage to MT545 sample-matched fields.
- Extended focused render metadata coverage to MT546 sample-matched fields.
- Extended focused render metadata coverage to MT547 sample-matched fields.
- Extended focused render metadata coverage to MT548 sample-matched fields.
- Extended focused render metadata coverage to MT549 sample-matched fields.
- Extended focused render metadata coverage to MT558 sample-matched fields.
- Extended focused render metadata coverage to MT564 sample-matched fields.
- Extended focused render metadata coverage to MT565 sample-matched fields.
- Extended focused render metadata coverage to MT566 sample-matched fields.
- Extended focused render metadata coverage to MT567 sample-matched fields.
- Extended focused render metadata coverage to MT568 sample-matched fields.
- Extended focused render metadata coverage to MT569 sample-matched fields.
- Extended focused render metadata coverage to MT575 sample-matched fields.
- Extended focused render metadata coverage to MT576 sample-matched fields.
- Extended focused render metadata coverage to MT578 sample-matched fields.
- Extended focused render metadata coverage to MT581 sample-matched fields.
- Extended focused render metadata coverage to MT586 sample-matched fields.
- Extended focused render metadata coverage to MT590 sample-matched fields.
- Extended focused render metadata coverage to MT591 sample-matched fields.
- Extended focused render metadata coverage to MT592 sample-matched fields.
- Extended focused render metadata coverage to MT595 sample-matched fields.
- Extended focused render metadata coverage to MT596 sample-matched fields.
- Extended focused render metadata coverage to MT598 sample-matched fields.
- Extended focused render metadata coverage to MT599 sample-matched fields.
- Extended focused render metadata coverage to MT670 sample-matched fields.
- Extended focused render metadata coverage to MT671 sample-matched fields.
- Added strict default parser limits with configurable lenient override and limit diagnostics.
- Added a swift-core cargo-fuzz harness and malformed-block parser smoke workflow.
- Added targeted malformed-tag fuzz coverage for swift-core text-field scanning.
- Added swift-core proptest coverage for generated balanced FIN message structures.
- Added a swift-core Criterion `parse_500kb` benchmark with zero-copy slice assertions for large synthesized FIN input.
- Documented and tested message-relative `ParseDiagnostic` byte offsets for swift-core `parse_message`.
- Added focused swift-api upload endpoint integration coverage through direct router injection.
- Added focused swift-api jobs-create endpoint integration coverage through direct router injection.
- Added focused swift-api jobs-list endpoint integration coverage through seeded direct router injection.
- Added focused swift-api job-status endpoint integration coverage through seeded direct router injection.
- Added focused swift-api manifest endpoint integration coverage through direct router injection.
- Added focused swift-api object endpoint integration coverage through direct router injection.
- Added focused swift-api metrics endpoint integration coverage through direct router injection.
- Added focused swift-api healthz endpoint integration coverage through direct router injection.
- Added focused swift-api readyz endpoint integration coverage through direct router injection.
- Added focused swift-api openapi endpoint integration coverage through direct router injection.
- Added configurable `/v1/*` request timeout middleware for swift-api with 504 timeout errors.
- Added per-client token-bucket rate limiting for swift-api write endpoints with 429 retry guidance.
- Added per-process `Idempotency-Key` replay for queued swift-api uploads.
- Added typed swift-api error-code mappings and documented stable error codes in OpenAPI.
- Added configurable prefix-job fanout caps for swift-api batch requests.
- Added comma-separated auth token rotation support for swift-api bearer auth.
- Added a real swift-api pending queue-depth gauge in Prometheus and JSON metrics.
- Added Prometheus registry-backed swift-api counters/gauges plus a job-duration histogram.
- Added swift-api in-process graceful-shutdown coverage that verifies queued jobs reach terminal state and workers drain.
- Added a swift-api Postgres system-of-record readiness probe so `/readyz` returns 503 until `SELECT 1` succeeds.
- Added swift-api integration coverage pinning the bounded job queue's 503 backpressure contract.
- Added strict unknown-field rejection for swift-api `JobRequest` JSON bodies and documented it in OpenAPI.
- Added swift-api OpenAPI consistency coverage that checks documented `JobRequest` fields against the request type's field list.
- Added early `Content-Length` enforcement for swift-api uploads while retaining body-size limits for chunked transfers.
- Added a structured `swiftpipe.parse_message` tracing span around the swift-api parse stage.
- Added a structured `swiftpipe.schema_materialize` tracing span around the swift-api schema matching and materialization stage.
- Added a structured `swiftpipe.duckdb_write` tracing span around swift-api DuckDB batch writes with row counts.
- Added a structured `swiftpipe.parquet_export` tracing span around swift-api Parquet exports with exported table and byte counts.
- Added a structured `swiftpipe.zip_create` tracing span around swift-api zip creation with file and byte counts.
- Added structured `swiftpipe.system_record_write` tracing spans around swift-api system-of-record writes.
- Added a structured `swiftpipe.http_request` tracing span that carries the propagated `x-request-id`.
- Added `swiftpipe_messages_processed_total{message_type=...}` Prometheus metrics for completed swift-api messages.
- Added `swiftpipe_parquet_bytes_written_total` Prometheus metrics for successful swift-api Parquet exports.
- Added `swiftpipe_object_store_ops_total{op=get|put|list}` Prometheus metrics for successful swift-api object-store operations.
- Added `swiftpipe_api_errors_total{code=...}` Prometheus metrics for typed swift-api errors.
- Added account-number redaction for swift-api internal error log fields.
- Added BIC redaction for swift-api internal error log fields.
- Replaced the swift-api job completion lifecycle log with a structured `swiftpipe.job_event` event.
- Switched swift-api `/metrics` responses to the OpenMetrics content type.
- Added `swiftpipe_jobs_total{status=queued|running|succeeded|failed|stuck}` Prometheus gauges for swift-api job lifecycle counts.
- Changed the swift-api default log filter to `swiftpipe_api=info,tower_http=warn` and added an observability runbook.
- Added a `swiftpipe_upload_body_bytes` Prometheus histogram for swift-api upload request sizes.
- Added a `swiftpipe_prefix_job_fanout_objects` Prometheus histogram for accepted prefix-job object counts.
- Added a `swiftpipe_duckdb_rows_written_total` Prometheus counter for successful swift-api DuckDB hydration writes.
- Added a CI hadolint job for `repo/Dockerfile` with documented temporary apt-version pinning ignore.
- Pinned Docker base images in `repo/Dockerfile` by digest, moved the Docker builder to Rust 1.88 for current dependency compatibility, and documented the re-pin policy in `repo/deploy/SECURITY.md`.
- Added a CI `docker-runtime-user` assertion that builds the runtime image and verifies it runs as UID 10001.
- Added a CI ShellCheck gate for `repo/scripts/*.sh` and fixed the existing script warnings.
- Pinned all `cargo install` invocations in CI, Dockerfile, and docs with exact versions plus `--locked`, and documented the policy in `repo/SECURITY.md`.
- Made swift-api CORS default-deny for cross-origin browser callers unless `SWIFTPIPE_CORS_ORIGINS` is explicitly configured.
- Added swift-api `--auth-required` / `SWIFTPIPE_AUTH_REQUIRED=1` startup enforcement that exits 78 unless `SWIFTPIPE_AUTH_TOKEN` is configured.
- Added swift-api tests that reject non-`s3://` job input URI, input prefix, and output prefix schemes at the API/job boundary.
- Added swift-api per-job zip export caps for total bytes and entry count, with typed `ZipLimitExceeded` failures and structured warning events.
- Added a swift-schema YAML alias-expansion DoS regression test that requires rejection within 500 ms.
- Pinned all GitHub Actions workflow `uses:` entries to immutable commit SHAs and documented the action upgrade process in `repo/SECURITY.md`.
- Added a Dependabot configuration for Cargo, Docker, and GitHub Actions dependency updates.
- Added a pinned CI SBOM job that generates and uploads a Syft SPDX-JSON artifact.
- Added a pinned CI Trivy image scan for the built Docker image, failing on HIGH/CRITICAL findings with an empty documented `.trivyignore`.
- Updated swift-api correlation-id middleware to honor valid client `x-request-id` values up to 128 characters and generate replacements otherwise.
- Added a Criterion bench for `swift-schema::infer_database_layout` over the full example schema corpus and documented the local baseline.
- Added a Criterion bench for `swift-db::materialize_message` against the representative MT540 sample and documented the local baseline.
- Added a Criterion bench for `swift-duckdb::DuckDbStore::write_batch` over a 10,000-row normalized batch and documented the local baseline.
- Added a Criterion bench for swift-api zip export over a 100-artifact local object-store prefix and documented the local baseline workflow.
- Pinned README-cited API throughput baselines and the current swift-core parser Criterion baseline in the benchmarking workflow.
- Added a CI quick-pass benchmark compile check with `cargo bench --workspace --no-run`.
- Added a swift-core parser allocation-audit benchmark and reduced parser hot-path temporary allocations with inline SmallVec buffers.
- Added a swift-schema renderer allocation-audit benchmark and moved renderer scratch buffers for matching rows, sequence paths, and sort keys to inline SmallVec storage.
- Added a configurable DuckDB parsed-output write batch size, defaulting to 1,000 rows per chunk and exposed through CLI sink config.
- Added a documented `--max-prefix-parallelism` cap for swift-api prefix jobs, defaulting to 4 worker threads per job.
- Added a CLI `--parquet-row-group-size` export flag that passes DuckDB `ROW_GROUP_SIZE` for Parquet exports while leaving CSV and API export behavior unchanged.
- Switched swift-api zip creation to stream `ZipWriter` output through a buffered temp file before atomic rename, preserving the existing zip limits and spans.
- Cached a validated swift-api `SchemaCatalog` in application state at startup so job requests no longer reload schema YAML from disk.
- Split CI workspace tests into a per-crate GitHub Actions matrix while keeping shared lint/check gates centralized.
- Added a CI coverage job that runs pinned `cargo-llvm-cov` and uploads the workspace LCOV report as a GitHub Actions artifact.
- Added a tag-triggered release workflow that publishes Linux CLI/API binary assets and pushes the swiftpipe-api Docker image to GHCR.
- Added `git-cliff` changelog configuration, a `CHANGELOG.md` skeleton, and release-note generation in the tag release workflow.
- Added cargo-dist workspace metadata for Linux/macOS binary release planning and a release-workflow `cargo dist plan` validation step.
- Added a CI rustdoc job that builds workspace docs with warnings denied.
- Added a Docker deployment workflow covering local container volumes, health checks, logs, environment, and upgrade steps.
- Added a minimal Helm chart for swift-api with image, replica, resources, service, probes, environment, and optional PVC values.
- Added a Postgres system-of-record workflow with disposable database validation and URL-style connection-string guidance.
- Added a schema authoring workflow with validated swift-cli schema checks and reproduction-test guidance.
- Added a spec reproduction workflow covering full checks, sequential replay, coverage audits, and reproduction-case authoring.
- Expanded the benchmarking workflow with result interpretation, common regression patterns, and bisect guidance.
- Added a data-corruption incident workflow for preserving evidence, reproducing Parquet exports on disposable DuckDB copies, and narrowing source/schema/materialization/export/downstream faults.
- Added a rate-limit saturation incident workflow covering 429 triage, client-key attribution, Retry-After handling, and safe rate-limit tuning.
- Added a backup/restore workflow for local object-root archives, Postgres dump/restore validation, restore ordering, and operational backup checks.
- Added a disaster recovery workflow for cold-starting from restored object storage and system-of-record backups.
- Added `SWIFTPIPE_LOG_FORMAT=json` for swift-api JSON log output and documented Vector/Fluent Bit log-shipping examples.
- Added a Docker Compose local stack for swift-api, Postgres system-of-record, and Grafana.
- Added a Docker Compose observability overlay with Loki, Tempo, Promtail, and Grafana datasource provisioning.
- Added a Kubernetes probe runbook documenting `/healthz` as liveness and `/readyz` as readiness, with local Helm validation commands.
- Added a Kubernetes pod resource runbook tying Helm CPU/memory defaults to local benchmark lessons and scale-up signals.
- Added ADR-0002 recording `s3://bucket/key` as the accepted object URI scheme for job inputs, outputs, manifests, and object retrieval.
- Added ADR-0003 recording SQLx as the implemented Postgres system-of-record sink while keeping DuckDB scoped to per-job hydration/export.
- Added ADR-0004 recording the bounded in-process Tokio job queue and HTTP 503 backpressure decision.
- Added ADR-0005 recording the local-disk-backed object store as the default behind the stable `s3://bucket/key` URI contract.
- Added ADR-0006 recording YAML as the schema authoring format, with validated catalogs as the boundary for future derived representations.
- Added ADR-0007 recording that render metadata lives beside parser metadata in the same schema field rules.
- Added ADR-0008 recording shared bearer-token auth as the production-default API authentication scheme.
- Added ADR-0009 recording `tracing` and Prometheus as SwiftPipe's observability substrate.
- Added ADR-0010 recording the six-crate Cargo workspace boundaries.
- Added ADR-0011 recording that API job render validation defaults to on.
- Enabled UI `noUncheckedIndexedAccess` TypeScript checking and added a CI UI
  build job that runs `npm ci` and `npm run build` in `repo/ui`.
- Added a UI ESLint flat-config baseline and wired `npm run lint` into the CI
  UI job before the production build.
- Added a Vitest/jsdom dashboard smoke test and wired `npm run test` into the
  CI UI job.
- Added a same-origin CSP meta policy to the UI HTML and removed the remaining
  inline React style so the policy does not require `style-src 'unsafe-inline'`.
- Added a CI UI runtime dependency audit gate with `npm audit --omit=dev`,
  which currently reports 0 runtime vulnerabilities.
- Added CI upload of the built `ui/dist` directory as the
  `swiftpipe-ui-dist` artifact.

## Active gaps

- Note: Earlier gaps about API harness, schema lifecycle docs, deployment
  guidance, and operational runbooks are superseded by the workflow docs,
  runbooks, CI gates, and ADRs indexed in [docs/index.md](../docs/index.md).
- Starter schemas and local reproduction assets still need careful wording:
  do not imply licensed, certified, or complete SWIFT coverage.
- Full UI dev-tool auditing still reports Vite/esbuild findings even though the
  runtime-only `npm audit --omit=dev` CI gate is clean.
- The queue runner still cannot commit completed work because `repo/` is not a
  Git repository in this workspace.

## Next likely tasks

- Keep context and docs synchronized as queued hardening lands.
- Resolve or explicitly document the UI dev-tool audit findings when a Vite
  major upgrade is acceptable.
- Continue treating any certified-message-coverage claims as out of bounds
  unless licensed source material is present.

## Known risks or fragile areas

- Schema fidelity and source licensing can become hidden constraints.
- Experimental status can be forgotten if not repeated in current-state docs.
- Latest local `swift-schema` layout inference bench passed but Criterion reported a statistically significant median slowdown (`487.27 µs`, `+8.1959%`) against its stored baseline; track before treating as a code regression.
- Full `repo/ui` dev-tool auditing still reports 2 moderate Vite/esbuild
  findings; the runtime-only `npm audit --omit=dev` gate is clean.
