# Postgres System-Of-Record Workflow

SwiftPipe can mirror job, audit, and artifact metadata into Postgres by running
the API with `--system-of-record postgres`. The sink creates the target schema
and tables when it first writes job metadata. `/readyz` checks Postgres with
`SELECT 1` before reporting the API ready.

Use a URL-style connection string:

```text
postgres://swiftpipe:swiftpipe@127.0.0.1:15432/swiftpipe
```

## Local Validation

Start a disposable Postgres container:

```bash
docker run -d --name swiftpipe-postgres-runbook -e POSTGRES_PASSWORD=swiftpipe -e POSTGRES_USER=swiftpipe -e POSTGRES_DB=swiftpipe -p 15432:5432 postgres:16
```

Wait for Postgres readiness:

```bash
docker exec swiftpipe-postgres-runbook pg_isready -U swiftpipe -d swiftpipe
```

Validate SQL access:

```bash
docker exec swiftpipe-postgres-runbook psql -U swiftpipe -d swiftpipe -c 'SELECT 1;'
```

Start SwiftPipe against Postgres:

```bash
cargo run -p swift-api -- --listen 127.0.0.1:18081 --schema-path examples/schemas --object-root .swiftpipe-objects --work-root .swiftpipe-work --system-of-record postgres --postgres-connection postgres://swiftpipe:swiftpipe@127.0.0.1:15432/swiftpipe --system-record-schema swiftpipe_runbook
```

From another shell, verify readiness:

```bash
curl -fsS http://127.0.0.1:18081/readyz
```

Expected response:

```json
{"status":"ok"}
```

Stop and remove the disposable database:

```bash
docker stop swiftpipe-postgres-runbook
docker rm swiftpipe-postgres-runbook
```

## Tables

The Postgres sink creates these tables under `--system-record-schema`:

- `swiftpipe_jobs`
- `swiftpipe_audit_events`
- `swiftpipe_artifacts`

The API upserts `swiftpipe_jobs`, appends audit rows, and upserts artifact
commit records. Keep this schema separate from application-owned tables so
SwiftPipe can manage its own metadata layout.

## Operations

Use a durable Postgres instance with backups enabled before turning this on in a
shared environment. Keep the connection string in the deployment secret manager,
not in command history or static manifests. In Kubernetes, pass it through a
Secret-backed environment variable and configure the API with
`SWIFTPIPE_POSTGRES_CONNECTION`.

The Postgres system-of-record mode is metadata-only. Original FIN inputs and
rendered/exported artifacts remain in object storage under the configured
`s3://`-mapped object root.
