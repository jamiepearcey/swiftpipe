# Kubernetes Pod Resources

SwiftPipe's Helm chart ships conservative starter resources for a single
`swiftpipe-api` pod:

```yaml
resources:
  requests:
    cpu: 250m
    memory: 512Mi
  limits:
    cpu: "1"
    memory: 2Gi
```

Use these defaults for trial deployments and low-volume validation. They are
based on the current local benchmark profile: parsing and schema matching are
CPU-heavy, DuckDB writes and Parquet export can create larger memory pressure,
and prefix jobs default to up to 4 worker threads per job.

## Starting Points

- Trial or development: request `250m` CPU and `512Mi` memory; limit at `1` CPU
  and `2Gi` memory.
- Small production workload: request `500m-1` CPU and `1-2Gi` memory; limit at
  `2` CPUs and `4Gi` memory.
- Prefix-heavy or export-heavy workload: request at least `1` CPU and `2Gi`
  memory; limit at `2-4` CPUs and `4-8Gi` memory after measuring the target
  schema, message size, output selection, and Parquet row-group size.

Keep `--max-prefix-parallelism` aligned with CPU limits. A pod capped at `1` CPU
should not run high fanout prefix jobs with the default 4 workers unless latency
is secondary to keeping memory bounded.

## Tuning Signals

Increase CPU requests when parser, schema, or materialization benchmarks remain
healthy but API job latency rises under concurrent uploads. CPU throttling is
visible in Kubernetes metrics as throttled container CPU seconds.

Increase memory requests or reduce export batch sizes when DuckDB writes,
Parquet exports, or zip exports coincide with memory pressure. The CLI
`--parquet-row-group-size` guidance in the benchmarking workflow starts at
`50,000-250,000` rows; use the lower end for constrained pods.

Do not set memory limits so low that normal DuckDB export spikes trigger OOM
restarts. Prefer sizing from measured peak resident memory plus headroom, then
use readiness and job-level controls to shed load before liveness restarts.

## Validate The Chart Defaults

Render the chart and inspect the resource stanza:

```bash
helm template swiftpipe repo/deploy/helm/swiftpipe | grep -A6 'resources:'
```

Confirm the chart remains valid after changing values:

```bash
helm lint repo/deploy/helm/swiftpipe
```

Record the workload shape, pod requests and limits, benchmark command, and
observed peak CPU/memory whenever you change production resource values.
