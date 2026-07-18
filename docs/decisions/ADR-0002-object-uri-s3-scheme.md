# ADR-0002: Use `s3://` URIs For Object Inputs And Outputs

## Status

Accepted

## Context

SwiftPipe jobs move FIN inputs, manifests, rendered messages, Parquet exports,
and zip artifacts through an object-store abstraction. The current local
implementation maps object URIs onto `--object-root`, but API requests and job
manifests still need a stable object identity that can survive a later move to a
remote object store.

Without a single scheme, callers and contributors could introduce `file://`,
local paths, `gs://`, or provider-specific URI styles in different endpoints.
That would complicate manifests, OpenAPI examples, job replay, backup/restore,
and path traversal controls.

The implemented code already treats `s3://bucket/key` as the object contract:

- `repo/crates/swift-api/src/object_store.rs` documents that all object URIs use
  `s3://bucket/key` regardless of backing implementation and rejects unsupported
  schemes.
- `repo/crates/swift-api/src/job.rs` parses job object URIs with the same
  `s3://` requirement before reading inputs or writing artifacts.
- `repo/crates/swift-api/src/routes.rs` rejects object retrieval requests that
  do not start with `s3://`.
- `repo/crates/swift-api/src/openapi.json` describes object parameters and job
  inputs using `s3://` examples.
- `repo/crates/swift-api/tests/jobs_create_endpoint.rs` and
  `repo/crates/swift-api/tests/object_endpoint.rs` cover rejection of non-`s3://`
  input and object URI schemes.

## Decision

Keep `s3://bucket/key` as the only supported object URI scheme for SwiftPipe job
inputs, input prefixes, output prefixes, manifests, artifacts, and object
retrieval.

The scheme names object-store objects, not necessarily the current storage
provider. Local development maps `s3://bucket/key` under
`--object-root/bucket/key`, while future remote implementations should preserve
the same external URI format.

## Consequences

- Manifests and API responses carry replayable object identities instead of
  process-local paths.
- The local object-store adapter can enforce bucket/key normalization and path
  traversal rejection behind one URI parser.
- Callers must translate local files into uploaded objects before submitting
  jobs, instead of passing `file://` URIs to job APIs.
- Adding another provider later requires an explicit ADR or compatibility layer,
  not ad-hoc endpoint-specific URI support.

## Alternatives Considered

- Accept local filesystem paths or `file://` URIs. Rejected because they leak
  deployment topology into API contracts and make manifests non-portable.
- Support multiple schemes such as `s3://`, `gs://`, and `az://` directly.
  Rejected for now because the current object-store abstraction and tests only
  need one stable provider-neutral object identity.
- Hide object locations behind opaque IDs only. Rejected because current
  backup, restore, replay, and object retrieval workflows benefit from explicit
  bucket/key paths in manifests and examples.
