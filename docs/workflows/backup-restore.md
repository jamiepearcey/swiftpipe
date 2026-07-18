# Backup And Restore

SwiftPipe has two durable data surfaces in production: the object root that maps
`s3://bucket/key` URIs to files, and the configured system-of-record database
when Postgres is enabled. The work root is operational scratch space unless an
operator explicitly stores file-backed audit records there.

## Backup Scope

Back up these paths or systems:

- `--object-root`: raw inputs, rendered outputs, Parquet exports, manifests,
  and zip artifacts.
- Postgres system-of-record database when `--system-of-record postgres` is
  enabled.
- File system-of-record JSONL path when `--system-of-record file` is enabled.
- Deployment configuration that names schema paths, object root, work root,
  Postgres connection, and system-record schema.

Do not treat `--work-root` as a replacement for object-root or Postgres
backups. Work-root contents may be temporary job-local files.

## Local Object-Root Backup

For a local filesystem object root, archive the object root from its parent
directory:

```bash
rm -rf /tmp/swiftpipe-backup /tmp/swiftpipe-restore /tmp/swiftpipe-object-root.tgz
mkdir -p /tmp/swiftpipe-backup/objects/bucket/key /tmp/swiftpipe-backup/work
printf 'sample\n' > /tmp/swiftpipe-backup/objects/bucket/key/sample.fin
tar -C /tmp/swiftpipe-backup -czf /tmp/swiftpipe-object-root.tgz objects
```

Restore into an empty destination and verify at least one expected object path:

```bash
mkdir -p /tmp/swiftpipe-restore
tar -C /tmp/swiftpipe-restore -xzf /tmp/swiftpipe-object-root.tgz
test -f /tmp/swiftpipe-restore/objects/bucket/key/sample.fin
```

For Kubernetes or Docker volume deployments, take the snapshot at the persistent
volume layer when available. If using tar, stop writers or take a filesystem
snapshot first so manifests and artifacts are captured consistently.

## Postgres Backup And Restore

Use a custom-format `pg_dump` so the backup can be restored into a fresh
database for validation:

```bash
docker rm -f swiftpipe-backup-postgres >/dev/null 2>&1 || true
docker run -d --name swiftpipe-backup-postgres -e POSTGRES_PASSWORD=swiftpipe -e POSTGRES_USER=swiftpipe -e POSTGRES_DB=swiftpipe postgres:16
until docker exec swiftpipe-backup-postgres pg_isready -U swiftpipe -d swiftpipe >/dev/null 2>&1; do sleep 1; done
docker exec swiftpipe-backup-postgres psql -U swiftpipe -d swiftpipe -c 'CREATE SCHEMA swiftpipe; CREATE TABLE swiftpipe.jobs(id text primary key); INSERT INTO swiftpipe.jobs VALUES ('\''job-1'\'');'
docker exec swiftpipe-backup-postgres sh -c 'pg_dump -U swiftpipe -d swiftpipe -Fc -f /tmp/swiftpipe.dump'
docker cp swiftpipe-backup-postgres:/tmp/swiftpipe.dump /tmp/swiftpipe.dump
```

Validate restore before trusting the backup:

```bash
docker exec swiftpipe-backup-postgres createdb -U swiftpipe swiftpipe_restore
docker cp /tmp/swiftpipe.dump swiftpipe-backup-postgres:/tmp/swiftpipe.dump
docker exec swiftpipe-backup-postgres pg_restore -U swiftpipe -d swiftpipe_restore /tmp/swiftpipe.dump
docker exec swiftpipe-backup-postgres psql -U swiftpipe -d swiftpipe_restore -c 'SELECT count(*) FROM swiftpipe.jobs;'
docker rm -f swiftpipe-backup-postgres
```

In production, run `pg_dump` against the managed Postgres instance or use the
provider's snapshot mechanism. Keep the SwiftPipe `--system-record-schema`
value with the backup record so restores can verify the expected schema.

## Restore Order

Restore object storage first, then restore Postgres, then start SwiftPipe with
the restored configuration. Verify readiness, submit a small known fixture, and
confirm the output manifest and system-record tables agree before resuming
normal traffic.

## Operational Checks

- Backup cadence matches the maximum acceptable data loss window.
- Restore is tested on a schedule, not only during incidents.
- Object-root backups and Postgres backups are labeled with compatible times.
- Backup storage is encrypted and access is limited to operators.
- Retention covers delayed downstream reconciliation and audit needs.
