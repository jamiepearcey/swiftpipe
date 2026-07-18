# Kubernetes Liveness And Readiness

SwiftPipe exposes separate HTTP probes so Kubernetes can distinguish a process
that should be restarted from a process that should temporarily stop receiving
traffic.

## Probe Mapping

- `/healthz` is the liveness endpoint. It returns whether the API process is
  alive and should stay running.
- `/readyz` is the readiness endpoint. It returns whether the API process is
  ready to accept requests.

The Helm chart maps these endpoints directly:

```yaml
readinessProbe:
  httpGet:
    path: /readyz
    port: http
livenessProbe:
  httpGet:
    path: /healthz
    port: http
```

## Operational Use

Use readiness for dependency or startup grace. When readiness fails, Kubernetes
removes the pod from Service endpoints without restarting the container. This is
the right behavior for cold starts, delayed object-store access, or future
system-of-record checks.

Use liveness only for conditions where restarting the process is the right
repair. Do not wire transient downstream dependency checks into liveness, or a
temporary outage can turn into a restart loop.

Keep probe timeouts conservative. The default chart values wait longer before
liveness starts than readiness starts, so a pod can warm up and become ready
before Kubernetes begins restart decisions.

## Validate The Chart

Run the chart validation before changing probe paths or timings:

```bash
helm lint repo/deploy/helm/swiftpipe
```

Render the deployment and inspect the probe paths:

```bash
helm template swiftpipe repo/deploy/helm/swiftpipe | grep -E 'readinessProbe|livenessProbe|path: /(readyz|healthz)'
```

Expected output includes `/readyz` under `readinessProbe` and `/healthz` under
`livenessProbe`.
