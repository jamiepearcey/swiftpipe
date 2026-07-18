# Disaster Recovery

Use this workflow to cold-start SwiftPipe from a backed-up object root and
system-of-record database after host, volume, cluster, or region loss.

The recovery objective is not to replay every historical job. It is to restore
the durable object namespace, restore the system of record, start SwiftPipe with
the same control-surface settings, and prove that new jobs can read restored
inputs and write new artifacts.

## Inputs Required

Before starting recovery, collect:

- Object-root backup or storage snapshot.
- Postgres backup when `--system-of-record postgres` was enabled.
- File system-of-record JSONL backup when `--system-of-record file` was enabled.
- Deployment configuration: schema path, object root, work root, auth settings,
  rate-limit settings, and system-record schema.
- Last known image tag or digest.
- Backup timestamp and expected recovery point.

If any durable surface is missing, record the missing artifact in the incident
record before starting partial recovery.

## Validate Runtime Flags

From `repo/`, confirm the API binary still exposes the recovery-relevant flags:

```bash
cargo run -p swift-api -- --help | grep -E -- '--object-root|--work-root|--system-of-record|--postgres-connection|--system-record-schema'
```

## Recovery Order

1. Provision clean object-root and work-root storage.
2. Restore the object-root backup into the object-root path.
3. Restore Postgres or the file system-of-record backup.
4. Restore deployment configuration and secrets.
5. Start SwiftPipe with the restored object root and system-of-record settings.
6. Check `/healthz` and `/readyz`.
7. Submit one representative known fixture or replay one preserved raw input.
8. Confirm the manifest, output artifacts, metrics, and system-record entries
   agree.
9. Reopen traffic gradually and watch queue depth, API errors, and job status.

Restore object storage before starting the API. Starting with an empty object
root can make old manifests and system-record entries point at missing
artifacts.

## Verification Checklist

- Restored object paths preserve the `s3://bucket/key` mapping under
  `--object-root/bucket/key`.
- The restored Postgres schema matches `--system-record-schema`.
- `/readyz` succeeds after object root, work root, schemas, and Postgres are in
  place.
- New jobs can read restored raw inputs and write to a new output prefix.
- A restored manifest can be fetched and its listed artifacts exist.
- Metrics show no sustained `rate_limited`, timeout, or queue-full error spike.
- Logs include normal job lifecycle events and no repeated object-store errors.

## Cutover Rules

Keep the recovered service read/write isolated until verification passes. If
callers can retry, preserve idempotency keys during cutover so repeated requests
do not create unnecessary duplicate jobs.

Do not delete the old environment, snapshots, or backup artifacts until the
incident owner confirms reconciliation is complete. If recovery involved a
partial backup, mark affected job IDs or object prefixes for downstream review.

## Follow-Up

After service is restored, update the incident record with actual recovery time,
last known good backup timestamp, any missing artifacts, and whether restore
validation found drift between object storage and system-of-record records.
