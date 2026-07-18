# System Overview

## Summary

Schema-driven SWIFT FIN ingestion engine for parsing, validating, normalizing, and rendering financial messages.

## What the system is for

Makes financial message ingestion easier to reason about by treating schemas as the central control surface.

## What is distinctive

Uses a schema-centric model that can support ingestion, validation, and rendering from the same representation.

## Major directories

- `repo/`: Main implementation.
- `notes/`: Working notes and planning.
- `research/`: Schema and ingestion experiments.

## Subsystem Flow

```text
CLI/API/UI request
  -> object-store input selection (`s3://bucket/key`)
  -> swift-core FIN parser
  -> swift-schema YAML catalog matching and validation
  -> swift-db normalized row materialization
  -> swift-duckdb per-job hydration/export
  -> schema-driven FIN rendering and artifact manifest
  -> local object-store outputs
  -> optional SQLx Postgres system-of-record events
```

The API wraps this flow with bearer-token auth, CORS allowlisting, request IDs,
rate limits, timeout handling, bounded queue backpressure, health/readiness
probes, tracing, and Prometheus metrics. The UI is a Vite/React control panel
served by the API for local operation.

## Ownership Boundaries

| Area | Location | Owns | Does not own |
| --- | --- | --- | --- |
| Parser | `repo/crates/swift-core` | FIN envelope/block/field parsing and parser diagnostics | Schema semantics or storage |
| Schema model | `repo/crates/swift-schema` | YAML schema loading, matching, field-type parsing, render metadata, render validation | API routing or DuckDB IO |
| Normalization | `repo/crates/swift-db` | Converting matched fields into normalized rows and inferred relational layout | DuckDB connection lifecycle |
| DuckDB bridge | `repo/crates/swift-duckdb` | Per-job DuckDB hydration, export, and render reads | Production system-of-record ownership |
| CLI | `repo/crates/swift-cli` | Local migration/run/render commands and schema checks | HTTP job queue behavior |
| API | `repo/crates/swift-api` | HTTP routes, object store, queue, job lifecycle, manifests, auth, metrics, optional Postgres event sink | Schema meaning and parser internals |
| UI | `repo/ui` | Browser control panel, job submission/status/detail, frontend lint/test/build | API persistence or ingestion semantics |
| Deployment | `repo/deploy`, `repo/Dockerfile`, `.github/workflows` | Container, Compose, Helm, CI, release, and operational validation | Business schema certification |

## Architecture Decisions

Current implementation constraints are recorded in ADR-0002 through ADR-0011
and summarized in [`.context/invariants.md`](../../.context/invariants.md).
Those records cover the `s3://` URI contract, Postgres system-of-record sink,
bounded queue, local object-store default, YAML schema authoring, render
metadata placement, bearer auth, observability substrate, crate boundaries, and
render-validation default.

## Notes

Note: Earlier placeholder notes to expand this file are superseded by the
subsystem flow, ownership table, and ADR links above.
