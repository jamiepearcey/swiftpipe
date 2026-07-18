# ADR-0009: Use Tracing And Prometheus For Observability

## Status

Accepted

## Context

SwiftPipe needs two complementary observability surfaces:

- structured event and span context for job lifecycle, HTTP requests, object
  operations, DuckDB writes, exports, and internal errors
- scrapeable numeric metrics for dashboards, alerts, and saturation tracking

The implemented API uses `tracing` and Prometheus:

- `repo/crates/swift-api/src/main.rs` initializes `tracing_subscriber` with the
  `RUST_LOG` env filter and `SWIFTPIPE_LOG_FORMAT=json` opt-in.
- `repo/crates/swift-api/src/routes.rs` creates structured
  `swiftpipe.http_request` spans and records typed API errors.
- `repo/crates/swift-api/src/job.rs` emits job, parse, materialization, system
  record, DuckDB, Parquet, render, zip, and lifecycle spans/events.
- `repo/crates/swift-api/src/state.rs` owns a Prometheus `Registry` with job,
  queue, object-store, message, Parquet, DuckDB, upload-size, prefix-fanout, and
  API-error metrics.
- `repo/crates/swift-api/src/routes.rs` exposes OpenMetrics-compatible text at
  `/metrics` and a JSON snapshot at `/metrics/json`.
- `repo/crates/swift-api/tests/metrics_endpoint.rs` verifies the metrics
  endpoint and individual metric families.
- `docs/runbooks/observability.md` and `docs/workflows/log-shipping.md`
  document log-level and JSON-log operations.

## Decision

Use `tracing` as the structured logging/span substrate and Prometheus metrics as
the numeric monitoring substrate.

Logs and spans carry causality and context. Prometheus metrics carry low-cardinal
aggregates suitable for alerting and dashboards. These surfaces should remain
coordinated but separate.

## Consequences

- Operators can collect text or JSON logs through standard log shippers while
  scraping `/metrics` for alertable signals.
- Code can add structured fields to spans without changing metric cardinality.
- Metrics must keep labels bounded and stable; high-cardinality details belong
  in tracing fields and job manifests.
- Prometheus/OpenMetrics output is available without a separate metrics service.
- Future OpenTelemetry tracing can build on `tracing` instrumentation instead
  of replacing application spans.

## Alternatives Considered

- `log` only. Rejected because plain logging lacks structured spans and async
  context propagation.
- `slog`. Rejected because `tracing` integrates more naturally with Tokio,
  Tower, spans, and the Rust async ecosystem.
- `metrics-rs`. Deferred because the current Prometheus crate gives direct
  registry control and simple endpoint encoding for the local API.
- OpenTelemetry-only instrumentation. Deferred because SwiftPipe needs a simple
  local default first; `tracing` keeps an upgrade path open.
