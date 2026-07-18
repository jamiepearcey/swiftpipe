# SwiftPipe Release Quality Plan

## Review Note

Claude Code was invoked headlessly twice for this review:

- `claude --print --permission-mode dontAsk --model sonnet --effort high ...`
- `claude --print --no-session-persistence --tools "" --model sonnet --effort medium ...`

Both processes launched but produced no output for several minutes and had to be
terminated. The plan below is based on direct review of the current workspace.

## Executive Summary

SwiftPipe has a strong parsing/materialization/rendering core and now has a first
API artifact slice, but the new API is not release-quality yet. The immediate
goal is to keep the hot path fast while establishing a durable artifact contract:
input object snapshot, per-message result records, DuckDB hydration, Parquet
dataset, render output, validation/error artifacts, and a final manifest written
last.

Release-quality v1 should remain self-hosted and vendor-decoupled:

- local object store now
- S3-compatible object store behind the same trait later
- no required Postgres
- optional job-store abstraction later for Postgres/control-plane durability

## Critical Findings

1. `crates/swift-api/src/main.rs` combines HTTP, object storage, job execution,
   manifest formatting, zip export, and UI into one file. This blocks focused
   testing and makes the API hard to harden.
2. Job execution fails the entire job on the first bad input object. Release
   behavior should produce a durable failed manifest/status and per-input result
   rows.
3. Artifact writes are not atomic. `manifest.json` is written after outputs, but
   status/error artifacts and object writes can still be partially written.
4. Rendered FIN uses a default envelope, not the original input envelope. This is
   acceptable for canonical outbound generation, but the harness requirement
   asks to parse and reserialize back to the original MT type. The API should at
   least preserve block 1/2/3/5 content from the source message for roundtrip
   rendering when available.
5. The HTTP implementation is a minimal blocking server. That is acceptable for
   a test harness but needs request-size limits, method handling, content-type
   clarity, and no accidental path escape.
6. Prefix jobs snapshot by listing at start, which is good, but the snapshot is
   currently only written inside the final successful path. It should be written
   before processing begins.
7. The object-store abstraction is implicit and local-only. It should become an
   explicit trait with a local backend now and S3-compatible backend later.
8. No API tests exercise prefix jobs, failed input handling, manifest failure
   state, zip contents, or object path traversal rejection.

## Incremental Implementation Steps

- [x] 1. Split the API into modules without changing behavior:
  - `http`
  - `object_store`
  - `job`
  - `manifest`
  - `ui`
  - Keep `main.rs` as wiring only.

- [x] 2. Make object writes atomic for local storage:
  - write to a same-directory temp file
  - rename into place
  - clean temp files on failure
  - use atomic writes for `status.json`, `input_snapshot.json`, `manifest.json`,
    `errors.ndjson`, rendered FIN, and zip.

- [x] 3. Create an explicit object-store interface:
  - `get(uri) -> bytes`
  - `put(uri, bytes)`
  - `put_atomic(uri, bytes)`
  - `list(prefix, selector)`
  - `local_path_for_export(uri)` only for local backend and DuckDB export
  - reject unsupported schemes clearly.

- [x] 4. Harden URI/path handling:
  - reject `..`, empty bucket, and absolute path injection in `s3://` URIs
  - keep `file://` disabled by default or behind an explicit `--allow-file-uris`
  - add unit tests for traversal attempts.

- [x] 5. Add durable job status artifacts:
  - write `status.json = running` before processing
  - write `input_snapshot.json` immediately after snapshot
  - write `status.json = completed` after manifest
  - write `status.json = failed` and `manifest.json` on job-level failure.

- [x] 6. Convert per-input failures into per-message/source results:
  - do not abort prefix jobs on one bad input
  - record `status=failed`, `input_uri`, and error
  - continue processing remaining inputs unless a fatal infrastructure error
    occurs.

- [x] 7. Preserve source envelope for API roundtrip rendering:
  - derive block 1/2/3/5 content from parsed input when available
  - fallback to configured/default envelope only if input block is missing
  - keep canonical block 4 from schema/data.

- [x] 8. Add API request limits:
  - max header size
  - max upload body size config
  - clear `413 Payload Too Large` response.

- [x] 9. Improve HTTP behavior:
  - `405 Method Not Allowed` for known paths with wrong method
  - structured JSON errors with stable codes
  - content types for parquet/fin/zip/ndjson.

- [x] 10. Add prefix job tests:
  - seed two local `s3://` inputs
  - submit `input_prefix`
  - assert deterministic `input_snapshot.json`
  - assert both rendered FIN outputs and manifest counts.

- [x] 11. Add failure manifest tests:
  - bad input among good inputs
  - missing schema/message type
  - invalid URI
  - ensure `status.json` and `manifest.json` are still written.

- [x] 12. Add zip artifact tests:
  - zip contains manifest, errors, rendered FIN, and parquet files
  - zip does not include itself
  - zip is readable after job completion.

- [x] 13. Improve harness UI:
  - show input URI, output prefix, manifest URI, zip link
  - show parse/render errors
  - support prefix job submission
  - support rendered FIN download.

- [x] 14. Add API documentation:
  - request/response examples
  - artifact contract
  - local object-store mapping
  - planned S3 backend contract
  - failure semantics.

- [x] 15. Performance pass:
  - avoid reading large objects into a single `Vec<u8>` where possible
  - benchmark parse/materialize/render for sample batches
  - document expected throughput and bottlenecks.

## Test Plan

- `cargo fmt --all`
- `cargo test -p swift-api -- --nocapture`
- `cargo test --workspace`
- `bash scripts/check-spec-reproduction.sh`
- API smoke test:
  - start `swiftpipe-api`
  - POST `examples/mt540_sample.fin` to `/api/upload?message_type=MT540`
  - fetch manifest
  - fetch rendered FIN
  - inspect generated Parquet and zip artifacts.

## Release Criteria

- API artifact contract is deterministic and documented.
- Successful jobs write `input_snapshot.json`, `status.json`, `manifest.json`,
  `normalized/*.parquet`, `errors.ndjson`, `rendered/*.fin`, and `exports.zip`.
- Failed jobs still write durable failure status and manifest.
- Prefix jobs process an immutable snapshot.
- One bad input in a prefix job does not destroy the whole batch.
- Rendered output preserves source envelope blocks when possible and canonical
  schema-rendered block 4.
- Local object-store paths cannot escape `--object-root`.
- Full workspace tests and spec reproduction checks pass.
