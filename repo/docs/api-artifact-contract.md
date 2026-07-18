# SwiftPipe API Artifact Contract

This API is a self-hosted artifact service for fast FIN MT ingestion,
normalization, Parquet export, and canonical outbound rendering. DuckDB is used
as a per-job hydration layer, not the durable system of record.

## Storage Model

The v1 API accepts object-style URIs and writes all durable inputs and outputs as
artifacts.

The original FIN input remains durable at the manifest `input_uris`. For speed,
the API does not duplicate full raw FIN bodies into the DuckDB hydration export
unless the server is started with `--persist-raw-text`; without that flag,
`swift_raw_messages` retains message id/type rows with empty `raw_text`.
Similarly, `swift_fields.raw_value` is empty by default; use
`--persist-raw-fields` when downstream consumers need field-level raw payloads.

Current backend:

- `s3://bucket/key` maps to `--object-root/bucket/key`
- only `s3://` URIs are accepted
- `file://`, `..`, empty buckets, and path traversal segments are rejected
- writes for durable artifacts are atomic on the local backend
- optional system-of-record sync writes job, audit, and artifact metadata to a
  configured sink; it is not part of the hot artifact path unless enabled

Planned backend:

- the same URI contract should be backed by an S3-compatible object store
- large payloads should move through object storage, not through the API process
- queue/job-store integrations should carry references to objects, not FIN bytes

## Job Artifacts

Every job writes under one output prefix, normally:

```text
s3://swiftpipe-outbox/jobs/{job_id}/
```

Successful or partially successful jobs produce:

```text
input_snapshot.json
status.json
manifest.json
errors.ndjson
rendered/msg-1.fin
rendered/msg-2.fin
normalized/*.parquet
exports.zip
```

Fatal job setup failures still write:

```text
status.json
manifest.json
```

`input_snapshot.json` is written before processing begins. Prefix jobs process
that immutable snapshot, so later objects added under the prefix are not part of
the running job.

## System Of Record

The system of record is a small repository/sink layer, separate from DuckDB
hydration. DuckDB remains a per-job artifact builder for Parquet; the system of
record tracks control-plane metadata:

- `swiftpipe_jobs`: current job status and counts
- `swiftpipe_audit_events`: append-only lifecycle events
- `swiftpipe_artifacts`: committed artifact URI records

Runtime modes:

```text
--system-of-record none
--system-of-record file --system-record-file .swiftpipe-work/system-record.jsonl
--system-of-record postgres --postgres-connection "host=... dbname=... user=..." --system-record-schema swiftpipe
--system-of-record sqlserver --sqlserver-connection "..."
```

`postgres` uses a SQLx-backed sink, creates the metadata tables if needed, and
upserts/appends via Postgres SQL. `sqlserver` is intentionally explicit but not
active yet; it returns an unsupported error until the ODBC/nanodbc adapter is
implemented.

## Endpoints

### `GET /`

Returns the embedded harness UI.

### `POST /api/upload?message_type=MT540`

Uploads one raw FIN MT message in the request body, stores it under the local
inbox object prefix, processes it, and returns the job manifest.

`message_type` is optional when block 2 can be inferred.
`outputs` is optional and accepts a comma-separated list. Supported values are
`rendered`, `parquet`, `errors`, `zip`, `manifest`, and `all`. Omit it for the
full default artifact set.

Example:

```bash
curl -sS \
  -X POST \
  "http://127.0.0.1:8080/api/upload?message_type=MT540&outputs=rendered" \
  --data-binary @examples/mt540_sample.fin
```

### `POST /api/jobs`

Starts an object-backed job.

Single object:

```json
{
  "input_uri": "s3://swiftpipe-inbox/manual/message.fin",
  "output_prefix": "s3://swiftpipe-outbox/jobs/manual-1/",
  "message_type": "MT540",
  "render_validate": true,
  "outputs": ["rendered"]
}
```

Prefix object snapshot:

```json
{
  "input_prefix": "s3://swiftpipe-inbox/daily/",
  "include_suffix": ".fin",
  "output_prefix": "s3://swiftpipe-outbox/jobs/daily-2026-05-22/",
  "message_type": "MT540",
  "render_validate": true,
  "outputs": ["parquet", "errors", "zip"]
}
```

DuckDB is only required for the `parquet` output. Jobs requesting only
`rendered` and/or `manifest` bypass DuckDB hydration and do not create a
temporary `.duckdb` workspace.

Response is a manifest.

### `GET /api/jobs/{job_id}/manifest`

Reads:

```text
s3://swiftpipe-outbox/jobs/{job_id}/manifest.json
```

This route is useful for default-output jobs. For custom `output_prefix` jobs,
read the manifest through `/api/object/{encoded-uri}`.

### `GET /api/object/{encoded-uri}`

Fetches an artifact by URL-encoded object URI.

Example:

```bash
curl -sS \
  "http://127.0.0.1:8080/api/object/s3%3A%2F%2Fswiftpipe-outbox%2Fjobs%2Fjob-1%2Fexports.zip" \
  -o exports.zip
```

Content types:

- `.json`: `application/json`
- `.ndjson`: `application/x-ndjson`
- `.fin`: `application/vnd.swift.fin`
- `.parquet`: `application/vnd.apache.parquet`
- `.zip`: `application/zip`

## Manifest Shape

```json
{
  "job_id": "uuid",
  "status": "completed",
  "processing_elapsed_ms": 685,
  "timings": {
    "setup_ms": 128,
    "prepare_ms": 147,
    "hydrate_write_ms": 372,
    "export_ms": 26,
    "manifest_ms": 0,
    "zip_ms": 8
  },
  "input_uris": ["s3://swiftpipe-inbox/uploads/uuid.fin"],
  "output_prefix": "s3://swiftpipe-outbox/jobs/uuid/",
  "outputs": {
    "manifest": "s3://swiftpipe-outbox/jobs/uuid/manifest.json",
    "normalized_parquet_prefix": "s3://swiftpipe-outbox/jobs/uuid/normalized/",
    "errors_ndjson": "s3://swiftpipe-outbox/jobs/uuid/errors.ndjson",
    "rendered_prefix": "s3://swiftpipe-outbox/jobs/uuid/rendered/",
    "zip": "s3://swiftpipe-outbox/jobs/uuid/exports.zip"
  },
  "counts": {
    "input_objects": 1,
    "messages": 1,
    "parse_errors": 0,
    "rendered": 1
  },
  "messages": [
    {
      "message_id": "msg-1",
      "message_type": "MT540",
      "input_uri": "s3://swiftpipe-inbox/uploads/uuid.fin",
      "status": "completed",
      "elapsed_ms": 312,
      "parse_errors": 0,
      "rendered_uri": "s3://swiftpipe-outbox/jobs/uuid/rendered/msg-1.fin",
      "error": null
    }
  ]
}
```

Statuses:

- `completed`: all selected inputs completed
- `completed_with_errors`: at least one selected input failed, but the job
  produced durable artifacts for the batch
- `failed`: fatal setup/export failure

## Error Responses

HTTP errors are JSON:

```json
{
  "status": "error",
  "code": "bad_request",
  "error": "request body must be a JSON job request"
}
```

Stable codes currently include:

- `bad_request`
- `not_found`
- `method_not_allowed`
- `payload_too_large`
- `internal_error`

## Operational Guidance

For fast production-style operation:

- prefer object URIs over large request bodies
- use prefix jobs for batch processing
- keep queues or schedulers outside the hot path and pass object references
- treat DuckDB files under `--work-root` as disposable hydration artifacts
- load downstream systems from `normalized/*.parquet`, `manifest.json`, and
  `errors.ndjson`

Postgres is not required for the core data path. If used, it should be an
optional control-plane job store for submission/status/audit, with object
storage remaining the durable artifact layer.

## Benchmarking

Use the API benchmark harness for repeatable local measurements:

```bash
./scripts/benchmark-api.sh --target-bytes 500M --object-bytes 4M
```

The harness:

- builds `swiftpipe-api`
- starts a local API server
- generates synthetic padded FIN MT objects under `.swiftpipe-bench/objects`
- submits a prefix job through `POST /api/jobs`
- writes JSON timing results under `.swiftpipe-bench/results`
- records the exact invocation, artifact URIs, and replayable curl commands in
  the result JSON

The default profile is `release`, which should be used for throughput numbers.
For a fast harness smoke test:

```bash
./scripts/benchmark-api.sh --profile debug --target-bytes 1M --object-bytes 256K
```

To test the large HTTP-body path as well:

```bash
./scripts/benchmark-api.sh --target-bytes 500M --object-bytes 4M --single-upload-bytes 500M
```

The single-upload benchmark intentionally pushes a large request body through
the API process. The preferred production path for large batches is still
object-prefix input, because it keeps big payloads out of the API transport.

To prepare a large corpus without starting the API, use:

```bash
./scripts/benchmark-api.sh --target-bytes 500M --object-bytes 4M --single-upload-bytes 500M --generate-only
```

That mode is useful for CI workers or locked-down laptops where the API binary
is supplied separately. For those runs, pass `--skip-build --api-binary
/path/to/swiftpipe-api` when executing the live benchmark.

The benchmark harness defaults to the API's fast hydration mode, where full raw
FIN text stays in object storage and is not duplicated into `swift_raw_messages`
Parquet rows. Add `--persist-raw-text` to benchmark the compatibility mode that
also writes full raw FIN text into DuckDB/Parquet. Add `--persist-raw-fields`
to retain `swift_fields.raw_value` payloads as well.

Measured locally in release mode on this machine:

| Path | Corpus | Result |
| --- | ---: | ---: |
| Core parser microbenchmark | sample FIN | ~509-514 MiB/s |
| API object-prefix, rendered-only bypass | 500 MiB / 125 objects | 0.15s, ~3424.7 MiB/s |
| API object-prefix, fast hydration | 500 MiB / 125 objects | 0.50-0.77s, ~651-1006 MiB/s |
| API object-prefix, persisted raw fields | 16 MiB / 4 objects | 0.38s, ~41.9 MiB/s |
| API object-prefix, persisted raw text and fields | 16 MiB / 4 objects | 0.52s, ~30.9 MiB/s |
| API single HTTP upload | 4 MiB | 1.12s, ~3.6 MiB/s |

These numbers are directional, not a public SLA. The important shape is that
object-prefix jobs are the fast path; large HTTP uploads are intentionally a
convenience path.
