# Log Shipping

SwiftPipe writes API logs to stdout/stderr. Keep log collection at the runtime
layer: Docker log driver, Kubernetes log collector, Vector, Fluent Bit, or the
platform's managed equivalent. Do not scrape files from `--work-root`.

## JSON Log Mode

Text logs remain the default. Enable JSON logs with `SWIFTPIPE_LOG_FORMAT=json`
and keep the normal filter explicit:

```bash
mkdir -p /tmp/swiftpipe-log-objects /tmp/swiftpipe-log-work
SWIFTPIPE_LOG_FORMAT=json RUST_LOG=swiftpipe_api=info,tower_http=warn cargo run -p swift-api -- --listen 127.0.0.1:18082 --schema-path examples/schemas --object-root /tmp/swiftpipe-log-objects --work-root /tmp/swiftpipe-log-work
```

JSON log events include fields such as `timestamp`, `level`, `target`,
`fields.message`, and structured job/request fields emitted by the API. Values
other than `json` fall back to text formatting.

## Vector Example

This Vector example tails JSON log files and parses each line before forwarding
to stdout. Replace the sink with the fleet destination.

```yaml
sources:
  swiftpipe_json:
    type: file
    include:
      - /var/log/swiftpipe/*.json
    read_from: beginning
transforms:
  parse_swiftpipe:
    type: remap
    inputs:
      - swiftpipe_json
    source: |
      . = parse_json!(.message)
sinks:
  stdout:
    type: console
    inputs:
      - parse_swiftpipe
    encoding:
      codec: json
```

## Fluent Bit Example

This Fluent Bit example tails the same JSON log files with the built-in JSON
parser and writes parsed records to stdout. Replace the output with the fleet
destination.

```ini
[SERVICE]
    Config_Watch Off
    Parsers_File /fluent-bit/etc/parsers.conf

[INPUT]
    Name tail
    Path /var/log/swiftpipe/*.json
    Parser json
    Tag swiftpipe

[OUTPUT]
    Name stdout
    Match swiftpipe
```

## Operational Guidance

- Prefer JSON logs in production and text logs for local debugging.
- Keep `tower_http` at `warn` unless investigating routing or timeout behavior.
- Preserve `x-request-id` and job IDs in the log sink index.
- Alert on sustained `rate_limited`, queue-full, timeout, and internal-error
  events through metrics first, then use logs for request/job context.
- Keep log retention aligned with manifest and system-of-record retention.
