# Docker Deployment Workflow

This workflow covers running the SwiftPipe API container locally or on a small
single-host Docker runtime. It assumes the image has already been built by CI or
pulled from the registry. The validation commands below were run locally against
`swiftpipe:check`; replace that image name with the release image tag for a real
deployment.

## Runtime directories

Create durable host directories for object storage and job work files:

```bash
mkdir -p .swiftpipe-objects .swiftpipe-work
```

Mount both directories into the container. The runtime image runs as the
`swiftpipe` user and writes to `/data/objects` and `/data/work`.

## Start the API

Run the container with the bundled schema corpus and UI:

```bash
docker run -d --name swiftpipe-local -p 18080:8080 -v "$PWD/.swiftpipe-objects:/data/objects" -v "$PWD/.swiftpipe-work:/data/work" swiftpipe:check
```

Validate readiness:

```bash
curl -fsS http://127.0.0.1:18080/healthz
```

Expected response:

```json
{"status":"ok"}
```

Confirm the container is using the expected mounts and runtime user:

```bash
docker inspect swiftpipe-local --format '{{.State.Status}} {{.Config.User}} {{range .Mounts}}{{.Destination}}={{.Source}} {{end}}'
```

## Environment

The default container command listens on `0.0.0.0:8080`, uses bundled schemas at
`/app/schemas`, writes objects to `/data/objects`, writes job work files to
`/data/work`, and serves the bundled UI from `/app/ui`.

Common environment settings:

- `RUST_LOG`: keep `swiftpipe_api=info,tower_http=warn` for normal operations.
- `SWIFTPIPE_AUTH_REQUIRED=1`: require bearer auth at startup.
- `SWIFTPIPE_AUTH_TOKEN`: comma-separated bearer tokens for API callers.
- `SWIFTPIPE_CORS_ORIGINS`: comma-separated browser origins allowed to call the
  API.

For custom schemas, mount a read-only schema directory and pass
`--schema-path /schemas` after the image name. Keep object and work directories
mounted separately so artifacts can be backed up and inspected independently.

## Logs

Docker captures the API's structured stdout/stderr stream. Tail local logs with:

```bash
docker logs --tail 20 swiftpipe-local
```

For production log shipping, configure the Docker daemon or container runtime
log driver to forward stdout/stderr to the fleet log sink. Keep SwiftPipe logs
structured at the source; do not scrape files from `/data/work`.

## Upgrade Procedure

Use a new immutable image tag or digest for each upgrade. Start the replacement
with the same volume mounts and environment, verify `/healthz`, then remove the
old container after traffic has moved.

For a single local container, stop the validated instance before starting the
replacement:

```bash
docker stop swiftpipe-local
```

If the replacement changes schemas or authentication policy, verify readiness
and one representative ingest job before deleting the previous image. Object and
work directories are host-mounted, so stopping the container does not remove
durable job artifacts.
