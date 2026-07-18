# SwiftPipe Productionisation Plan

Scope decisions driving this plan (agreed 2026-05-25):

- **Deployment model: single-tenant appliance.** One organisation runs its own
  instance behind its own network boundary. Multi-tenant isolation is therefore
  out of scope, but the service still must not be trivially DoS-able or expose
  its entire object root unauthenticated.
- **Correctness scope: structural round-trip only.** Outputs are consumed by
  data pipelines, not submitted to the live SWIFT network. Full SWIFT network
  validated-rule reproduction (SRU/ISO licensed material) is **deferred** and
  tracked as the top open risk, not a release gate.
- This document complements `release-quality-plan.md`, which covered the
  artifact-contract slice. That checklist is complete; this plan covers turning
  the service into something operable in production.

## Executive Summary

The engine (parse → schema match → materialize → render) is solid and well
tested. The **service wrapper is demo-grade**: a single-threaded, blocking,
hand-rolled HTTP server that executes entire jobs synchronously inside the
request handler, with no authentication, no streaming, no concurrency, and no
operational instrumentation. Productionising is overwhelmingly about the
`swift-api` crate and its delivery/runtime, not the core libraries.

Given the single-tenant appliance scope, the ordered priorities are:

1. **Service foundation** — concurrency, timeouts, streaming, health, graceful
   shutdown, structured logging/metrics (removes the DoS surface).
2. **Async job execution** — submit/poll, worker pool, crash recovery, cleanup.
3. **Appliance-grade hardening** — a shared auth token, CORS lockdown,
   server-derived output prefixes, secrets off the command line.
4. **Storage + durability** — real S3-compatible backend, artifact GC, a proper
   pooled Postgres control-plane client.
5. **Delivery & operations** — CI, hardened Docker image, load testing, runbooks.

Deferred (documented risk, not gating): full SWIFT network-rule validation.

## Critical Findings (Blockers)

### 1. Single-threaded blocking server
`crates/swift-api/src/main.rs:79` handles connections serially via
`for stream in listener.incoming()`. No thread pool, no async runtime. One client
blocks all others. With no socket read timeout in `read_http_request`
(`crates/swift-api/src/http.rs:71`), a single idle/slow client (Slowloris) wedges
the whole service. Disqualifying on its own.

### 2. Jobs run synchronously inside the HTTP request
`crates/swift-api/src/routes.rs:37` runs `process_upload` / `process_job_request`
inline and returns the finished manifest as the 200 body. A large prefix job
holds the connection for its entire duration. There is no `202 + job_id` submit,
no queue, no worker pool, no in-progress polling, no cancellation.

### 3. No authentication and global object read exposure
`GET /api/object/{uri}` (`crates/swift-api/src/routes.rs:61`) serves any object
under `--object-root` to anyone who can reach the port. Path traversal is
rejected (`crates/swift-api/src/object_store.rs:79`) but global reads of the
inbox and every job's outputs are not. `output_prefix`/`input_prefix` are fully
caller-controlled (`JobRequest`), so a request can overwrite another job's
artifacts. CORS `Access-Control-Allow-Origin: *` is hardwired
(`crates/swift-api/src/http.rs:165`). Even for an appliance, this should require
a shared token and a locked-down CORS policy.

### 4. Everything is buffered fully in memory
Request bodies accumulate into a `Vec<u8>` (`crates/swift-api/src/http.rs:146`)
and objects are read whole via `fs::read` (`crates/swift-api/src/object_store.rs:18`).
No streaming. The 500 MiB single-upload path holds the whole payload in RAM,
contradicting the "move large payloads through object storage" guidance.

### 5. Hand-rolled, incomplete HTTP
No keep-alive, chunked transfer-encoding, `Expect: 100-continue`, or HTTP/2;
always `Connection: close`. `percent_decode` (`crates/swift-api/src/http.rs:212`)
has two real bugs: `index + 2 < bytes.len()` drops a valid `%XX` at end of input
(off-by-one), and `hex as char` / `bytes[index] as char` corrupts multibyte
UTF-8 in keys and URIs.

## Important Findings

### 6. Control-plane durability is fragile
`open_system_of_record` is called fresh per job and again in the failure path
(`crates/swift-api/src/job.rs:174`). For Postgres that means a new SQLx pool per
job (`crates/swift-api/src/system_record.rs:199`), with separate remote writes
and no job-wide transaction. Pool lifetime, batching, and transaction boundaries
still need production hardening.

### 7. No crash recovery
A job writes `status=running` up front; if the process dies mid-job that row
stays `running` forever. No lease/heartbeat/timeout. Disk artifacts and the
control plane diverge silently if a SoR write fails after a disk write.

### 8. No idempotency
Re-submitting an upload mints a new `job_id` and re-runs all work; no
idempotency key or dedup.

### 9. Resource leaks
Per-job `hydrate.duckdb` work dirs (`crates/swift-api/src/job.rs:248`) are never
cleaned up (only the rendered-only path avoids creating them). No retention/GC
for outbox artifacts. Disk grows unbounded.

### 10. Observability is print statements
Only `println!`/`eprintln!` (`crates/swift-api/src/main.rs:83`,
`crates/swift-api/src/job.rs:442`). No structured logs, request IDs, levels,
metrics, or tracing. No `/healthz` / `/readyz`; the Dockerfile has no
`HEALTHCHECK`. No graceful shutdown on SIGTERM — container stop kills the
in-flight job and orphans its `running` row and work dir.

### 11. No delivery pipeline
No `.github/` / CI. README documents manual `cargo fmt`/`cargo test` only. No
clippy gate, `cargo audit`/`cargo deny`, SBOM, or license scan. The Dockerfile
has no dependency-cache layer (every source change rebuilds all deps including
bundled DuckDB C++), floats on `rust:1-bookworm`, and has no healthcheck.

### 12. No API/contract versioning
No `/v1` prefix and no `contract_version` in the manifest, so the artifact
contract cannot evolve safely.

### 13. Secrets on the command line
The Postgres connection string (with password) is a CLI flag
(`crates/swift-api/src/main.rs:53`), leaking via `ps`, shell history, and
`docker inspect`.

## Strengths to Preserve

- Zero-copy, well-tested parser core (`crates/swift-core/src/lib.rs`).
- DuckDB inserts use parameterized statements; Postgres values are escaped.
- Atomic temp-file + rename writes (`crates/swift-api/src/object_store.rs:111`).
- Per-input failures already do not abort a prefix batch.
- The prepare step is already parallelized with `thread::scope`
  (`crates/swift-api/src/job.rs:567`).
- Object-store access already goes through a single struct, ready to become a
  trait with multiple backends.

## Phased Plan

### Phase 1 — Service foundation (highest priority)
Replace the hand-rolled server with `axum`/`hyper` on `tokio`. Gains:
connection concurrency, read/write/idle timeouts, body-size limits as
middleware, streaming bodies, keep-alive. Add:

- `/healthz` (liveness) and `/readyz` (schema catalog loaded, work/object roots
  writable).
- Graceful shutdown: drain in-flight work on SIGTERM/SIGINT.
- `tracing` with JSON logs, log levels, and a per-request correlation ID.
- A `/metrics` endpoint (job counts, durations, bytes processed, error rates).
- Version routes under `/v1`; add `contract_version` to the manifest.
- Fix or delete `percent_decode` in favour of a vetted crate.

### Phase 2 — Asynchronous job execution
- Submit returns `202 { job_id, status: "queued" }`; a bounded worker pool runs
  jobs off the request path.
- `GET /v1/jobs/{id}` returns real `queued`/`running`/`completed`/`failed`
  state from `status.json` + control plane.
- Idempotency keys so a retried submit returns the existing job.
- Job leases + heartbeat; a reaper marks abandoned `running` jobs `failed`.
- Cancellation endpoint.
- Clean up `work_root/{job_id}` on completion (and on failure).

### Phase 3 — Appliance-grade hardening
- Single shared bearer token (`--auth-token` via env/secret file) required on
  all `/api` routes except health; constant-time comparison.
- Lock CORS to a configured allowlist (default: none / same-origin UI only).
- Derive `output_prefix` server-side from `job_id`; treat any client-supplied
  prefix as advisory and confined to the outbox bucket. Reject `input_prefix`
  pointing at the outbox.
- Move all secrets to env vars / secret files; never CLI flags.
- TLS: either terminate in-process (rustls) or document the required reverse
  proxy and bind to loopback by default.

### Phase 4 — Storage backend
- Promote `LocalObjectStore` to an `ObjectStore` trait; add a real
  S3-compatible backend with streaming get/put and multipart upload.
- Rename the `s3://` scheme handling so a local backend is not labelled `s3://`,
  or make `s3://` actually mean S3 and add a `file://`/`local://` scheme for the
  local appliance store.
- Artifact lifecycle: configurable retention + a GC sweep for old outbox jobs.

### Phase 5 — Control-plane durability
- Replace the DuckDB→Postgres bridge with `sqlx`/`tokio-postgres`: connection
  pool, versioned migrations, parameterized + batched + transactional writes.
- Add reconciliation between on-disk artifacts and control-plane rows.
- Either finish the SQL Server adapter or remove the flag (it currently errors
  at runtime — `crates/swift-api/src/system_record.rs:119`).

### Phase 6 — Delivery & operations
- CI on every change: `fmt --check`, `clippy -D warnings`,
  `cargo test --workspace`, `cargo audit`, `cargo deny`, the spec-reproduction
  scripts, and an API integration/smoke test.
- Hardened Dockerfile: cargo-chef dependency caching, pinned base image digest,
  `HEALTHCHECK`, generated SBOM, non-root verified at the mounted data paths.
- Load/soak testing against agreed appliance SLOs; dashboards + alerts on the
  Phase 1 metrics; runbooks for restart, backlog, and disk-pressure scenarios.

### Deferred — Full SWIFT network compliance (tracked risk)
Out of scope under the structural-round-trip decision. If outputs ever need to
reach the live network or be certified, this becomes a gating workstream:
import licensed SR/ISO material, build a network validated-rule engine
(mandatory fields/sequences, field length/format, charset X/Y/Z, T/C/D rules),
and stand up a conformance corpus. Until then, keep the README's "starter
schemas, not certified" caveat prominent and avoid presenting structural
re-parse as compliance.

## Suggested First Slice
Phase 1 (server rewrite) + Phase 2 (async submit/poll) + the Phase 3 shared
token and CORS lockdown, delivered together. That single coherent change removes
the DoS surface, the synchronous-blocking model, and the unauthenticated global
object read — the three issues that most clearly block running this as an
appliance.

## Validation
- `cargo fmt --all --check`
- `cargo clippy --workspace -- -D warnings`
- `cargo test --workspace`
- `bash scripts/check-spec-reproduction.sh`
- API integration test: start server, submit upload + prefix job, poll status to
  completion, fetch manifest and artifacts, assert auth is enforced and a slow
  client cannot block a concurrent request.
