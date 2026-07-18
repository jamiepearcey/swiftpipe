# SwiftPipe Helm Chart

Validate the chart before deployment:

```bash
helm lint repo/deploy/helm/swiftpipe
```

Render manifests locally:

```bash
helm template swiftpipe repo/deploy/helm/swiftpipe
```

Minimum production values to review before install:

- `image.repository`, `image.tag`, and `image.pullPolicy`
- `replicaCount`
- `resources.requests` and `resources.limits`
- `persistence.enabled` and PVC sizes
- `env.authRequired`, `env.authTokenSecretName`, and `env.corsOrigins`

The chart defaults to `emptyDir` volumes for trial deployments. Enable
`persistence.enabled` before relying on job artifacts across pod restarts.
