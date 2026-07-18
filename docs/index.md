# swiftpipe Docs Index

## Start here for agents

Read these files before making changes:

- [docs/index.md](docs/index.md)
- [.context/project-brief.md](.context/project-brief.md)
- [.context/current-state.md](.context/current-state.md)
- [.context/invariants.md](.context/invariants.md)
- [docs/tasks/current.md](docs/tasks/current.md)
- [Architecture overview](architecture/system-overview.md)
- [Workflow instructions](workflows/agent-instructions.md)
- [Testing workflow](workflows/testing.md)
- [Benchmarking workflow](workflows/benchmarking.md)
- [Schema authoring workflow](workflows/schema-authoring.md)
- [Spec reproduction workflow](workflows/spec-reproduction.md)
- [Data corruption incident workflow](workflows/incident-data-corruption.md)
- [Rate-limit saturation incident workflow](workflows/incident-rate-limit-saturation.md)
- [Backup and restore workflow](workflows/backup-restore.md)
- [Disaster recovery workflow](workflows/disaster-recovery.md)
- [Log shipping workflow](workflows/log-shipping.md)
- [Docker deployment workflow](workflows/deploy-docker.md)
- [Docker Compose local stack](../repo/deploy/docker-compose.yml)
- [Docker Compose observability stack](workflows/compose-observability.md)
- [Kubernetes probe workflow](workflows/kubernetes-probes.md)
- [Kubernetes pod resource workflow](workflows/kubernetes-resources.md)
- [Postgres system-of-record workflow](workflows/postgres-system-of-record.md)
- [SwiftPipe Helm chart](../repo/deploy/helm/swiftpipe/README.md)
- [Observability runbook](runbooks/observability.md)
- [Decision record](decisions/ADR-0001-project-memory-structure.md)
- [ADR-0002: `s3://` object URI scheme](decisions/ADR-0002-object-uri-s3-scheme.md)
- [ADR-0003: Postgres system-of-record SQLx sink](decisions/ADR-0003-postgres-system-record-sqlx-sink.md)
- [ADR-0004: Bounded in-process job queue](decisions/ADR-0004-bounded-in-process-job-queue.md)
- [ADR-0005: Local-disk object-store default](decisions/ADR-0005-local-disk-object-store-default.md)
- [ADR-0006: YAML schema authoring format](decisions/ADR-0006-yaml-schema-authoring-format.md)
- [ADR-0007: Render metadata in schema files](decisions/ADR-0007-render-metadata-in-schema-files.md)
- [ADR-0008: Shared bearer-token auth](decisions/ADR-0008-shared-bearer-token-auth.md)
- [ADR-0009: Tracing and Prometheus observability](decisions/ADR-0009-tracing-prometheus-observability.md)
- [ADR-0010: Six-crate Cargo workspace](decisions/ADR-0010-six-crate-cargo-workspace.md)
- [ADR-0011: Render validation default-on](decisions/ADR-0011-render-validation-default-on.md)

## Major directories

- `repo/`: Main implementation.
- `notes/`: Working notes and planning.
- `research/`: Schema and ingestion experiments.

## Context files

- [Project brief](../.context/project-brief.md)
- [Current state](../.context/current-state.md)
- [Invariants](../.context/invariants.md)

## Workflows and runbooks

- [Agent instructions](workflows/agent-instructions.md)
- [Testing](workflows/testing.md)
- [Benchmarking](workflows/benchmarking.md)
- [Schema authoring](workflows/schema-authoring.md)
- [Spec reproduction](workflows/spec-reproduction.md)
- [Docker deployment](workflows/deploy-docker.md)
- [Docker Compose observability](workflows/compose-observability.md)
- [Postgres system of record](workflows/postgres-system-of-record.md)
- [Kubernetes probes](workflows/kubernetes-probes.md)
- [Kubernetes resources](workflows/kubernetes-resources.md)
- [Log shipping](workflows/log-shipping.md)
- [Backup and restore](workflows/backup-restore.md)
- [Disaster recovery](workflows/disaster-recovery.md)
- [Data corruption incident](workflows/incident-data-corruption.md)
- [Rate-limit saturation incident](workflows/incident-rate-limit-saturation.md)
- [Observability runbook](runbooks/observability.md)

## Work queues

- [Backlog](tasks/backlog.md)
- [Current work](tasks/current.md)
- [Task template](tasks/task-template.md)
