# Incident: Rate Limit Saturation

Use this checklist when `swiftpipe_api_errors_total{code=rate_limited}` spikes
or clients report HTTP 429 responses from write endpoints.

SwiftPipe rate limits write routes by client key. The key is the first
`x-forwarded-for` value when present, otherwise the peer IP address. A 429
response includes `code: rate_limited` in the JSON body and a `Retry-After`
header.

## Triage

- Confirm the spike is `code=rate_limited`, not queue saturation or request
  timeout.
- Check which write endpoint is affected: upload, job creation, or another
  `/v1/*` write route.
- Group recent requests by client IP or the first `x-forwarded-for` hop.
- Confirm whether the spike is one client, a NAT/proxy group, or all clients.
- Check whether clients are honoring `Retry-After` before retrying.
- Compare rate-limit errors with accepted job counts and queue depth to avoid
  raising limits while the worker queue is already saturated.

## Validate Local Rate-Limit Wiring

From `repo/`, confirm the deployed binary exposes the rate-limit knobs:

```bash
cargo run -p swift-api -- --help | grep -E -- '--rate-limit-(rps|burst)'
```

Confirm the regression test still exercises the 429 path:

```bash
cargo test -p swift-api write_rate_limit_returns_too_many_requests_when_bucket_is_exhausted
```

## Interpret The Spike

If one client key dominates, contact that caller first. Ask for request rate,
retry policy, idempotency-key behavior, and whether they recently changed batch
size or parallelism.

If many unrelated client keys spike at once, check deployment or routing first:
proxy `x-forwarded-for` handling, load-balancer source NAT, rollout timing,
auth failures causing retry loops, and synthetic monitoring frequency.

If `Retry-After` is absent, stale, or ignored by clients, treat that as a client
coordination issue before increasing capacity. Well-behaved clients should back
off, preserve idempotency keys for retries, and avoid retry storms.

If rate-limit errors rise together with queue depth, keep the rate limit in
place and reduce arrival rate. Raising the rate limit can move the failure from
429 responses to queue saturation, longer latency, or job timeouts.

## Mitigation

The API defaults are 5 requests per second with a burst of 20 per client key.
Increase limits only when the worker queue and object store have spare capacity,
and record the old and new values.

Prefer targeted mitigations:

- Fix proxy client-IP forwarding if many callers collapse to one key.
- Reduce client concurrency or add client backoff when one caller dominates.
- Increase `--job-workers` or queue capacity only when processing capacity is
  the actual bottleneck.
- Raise `--rate-limit-rps` or `--rate-limit-burst` only after confirming the
  extra accepted writes will not saturate downstream storage.

## Recovery Criteria

Close the incident when `rate_limited` error rate returns to expected baseline,
accepted job counts are stable, queue depth is not growing, and the affected
clients confirm they are no longer seeing sustained 429 responses.

Record the root cause, client keys involved, whether `x-forwarded-for` was
trusted, any rate-limit flag changes, and any client retry changes.
