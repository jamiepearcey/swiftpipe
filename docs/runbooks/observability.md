# Observability Runbook

## API log level

SwiftPipe API defaults to:

```bash
RUST_LOG=swiftpipe_api=info,tower_http=warn
```

This keeps SwiftPipe lifecycle, metric, and structured job events visible while
reducing routine tower HTTP request noise. Raise tower HTTP logging only during
request-routing investigations:

```bash
RUST_LOG=swiftpipe_api=info,tower_http=info
```

For normal local startup, keep the default filter explicit in the shell so the
process environment is visible in terminal history:

```bash
RUST_LOG=swiftpipe_api=info,tower_http=warn cargo run -p swift-api -- \
  --listen 127.0.0.1:8080 \
  --schema-path examples/schemas \
  --object-root .swiftpipe-objects \
  --work-root .swiftpipe-work
```
