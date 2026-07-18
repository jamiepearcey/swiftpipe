# ADR-0004: Use A Bounded In-Process Job Queue

## Status

Accepted

## Context

SwiftPipe upload and prefix endpoints can accept work faster than parsing,
materialization, rendering, object writes, and exports can complete. The API
therefore needs backpressure that protects memory and keeps latency visible to
callers.

The implemented queue is in-process and bounded:

- `repo/crates/swift-api/src/queue.rs` wraps a Tokio bounded `mpsc` channel and
  a worker pool.
- `JobQueue::submit` uses `try_send`, returning an error immediately when the
  queue is full or shut down.
- `repo/crates/swift-api/src/routes.rs` maps submit failures to
  `service_unavailable` API errors with HTTP 503.
- `repo/crates/swift-api/src/main.rs` exposes `--job-workers` and
  `--job-queue-capacity` as deployment knobs.
- `repo/crates/swift-api/tests/queue_full.rs` verifies that a queued upload
  returns HTTP 503 when the queue is saturated.
- `repo/crates/swift-api/src/state.rs` exposes queue depth and job-status
  metrics for saturation monitoring.

## Decision

Use a bounded in-process Tokio `mpsc` job queue for accepted API work. When the
queue is full, return HTTP 503 instead of blocking request handlers, allocating
unbounded memory, or silently dropping work.

Keep external durable queues out of the default architecture until the current
single-process job lifecycle needs cross-process scheduling, replay after API
crash, or multi-replica work distribution.

## Consequences

- Backpressure is immediate and visible to clients through HTTP 503.
- Memory use is bounded by queue capacity, request body limits, and worker
  concurrency.
- Operators can tune throughput and latency with worker count and queue
  capacity, then observe queue depth and job-status metrics.
- Queued jobs are not durable across process crashes; durable replay remains
  tied to object storage and system-of-record metadata, not the in-memory queue.
- Multiple API replicas each own an independent queue unless a future external
  scheduler is introduced.

## Alternatives Considered

- Use an unbounded in-process queue. Rejected because it hides overload until
  memory pressure or latency becomes operationally dangerous.
- Block request handlers until queue capacity is available. Rejected because it
  ties client connection lifetimes to worker throughput and makes overload less
  explicit.
- Introduce an external queue such as SQS, NATS, Kafka, or Postgres advisory
  jobs now. Deferred because the current implementation is a local vertical
  slice and does not yet need cross-process scheduling or durable queue replay.
- Process every request synchronously by default. Rejected for normal API
  operation because longer jobs would hold request handlers open and reduce
  admission control. Synchronous paths remain useful for tests and explicit
  workflows.
