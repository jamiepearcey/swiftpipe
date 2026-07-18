# ADR-0005: Default To A Local-Disk Object Store

## Status

Accepted

## Context

SwiftPipe needs durable storage for raw FIN inputs, manifests, rendered FIN
outputs, Parquet exports, error files, and zip artifacts. The external URI
contract is `s3://bucket/key`, but the current deployment target is still an
experimental local vertical slice where operators need simple, inspectable
storage without cloud credentials.

The implemented object-store path is local disk backed:

- `repo/crates/swift-api/src/object_store.rs` defines the `ObjectStore` trait
  and `LocalObjectStore`.
- `LocalObjectStore` maps `s3://bucket/key` to `--object-root/bucket/key`.
- `LocalObjectStore::put_atomic` writes through a temporary file and rename so
  partial outputs are not visible.
- `LocalObjectStore::local_path` rejects unsupported schemes, invalid buckets,
  path traversal segments, and backslash path segments.
- `repo/crates/swift-api/src/job.rs` and `repo/crates/swift-api/src/routes.rs`
  instantiate `LocalObjectStore` from application state for job processing and
  object retrieval.
- `repo/crates/swift-api/src/object_store.rs` tests URI mapping, path traversal
  rejection, atomic replacement, and outbox garbage collection.

## Decision

Use the local-disk-backed `LocalObjectStore` as the default SwiftPipe object
store while preserving the external `s3://bucket/key` URI contract.

The upgrade path to real S3 is to add a new implementation of the existing
`ObjectStore` trait that keeps the same URI format, object semantics, and API
manifests. Callers should not pass local paths or provider-specific schemes to
job APIs.

## Consequences

- Local development, Docker Compose, and small single-host deployments can run
  without cloud credentials.
- Operators can inspect and back up object data directly under `--object-root`.
- The API contract stays vendor-neutral from the caller's perspective, even
  though the default backing store is local disk.
- Real S3 support will require explicit implementation and operational work for
  credentials, consistency assumptions, retry behavior, multipart writes, and
  lifecycle policies.
- Multi-host deployments must use a durable shared object root or wait for a
  real remote object-store adapter before scaling job workers across hosts.

## Alternatives Considered

- Require real S3 from the start. Rejected because it would make the local
  vertical slice harder to run and test, and would require credentials for basic
  development.
- Expose `file://` or absolute local paths to callers. Rejected because it would
  make manifests deployment-specific and conflict with ADR-0002's stable object
  URI scheme.
- Add several provider adapters immediately. Deferred until the local API and
  artifact contract are stable enough to justify provider-specific operational
  semantics.
