# ADR-0008: Use Shared Bearer-Token Auth For Production

## Status

Accepted

## Context

SwiftPipe exposes job submission, artifact retrieval, metrics, and operational
endpoints over HTTP. Local development benefits from zero-config startup, but
production deployments need an authentication scheme that is simple to operate
before tenant-specific identity, mTLS, or gateway integrations are mature.

The implemented authentication surface is shared bearer-token auth:

- `repo/crates/swift-api/src/auth.rs` implements `AuthLayer` for
  `Authorization: Bearer <token>` checks.
- `SWIFTPIPE_AUTH_TOKEN` configures one or more comma-separated tokens for
  rotation.
- Token comparison uses constant-time byte comparison for equal-length tokens.
- `/healthz` and `/readyz` bypass auth so load balancers and Kubernetes probes
  can work without credentials.
- `repo/crates/swift-api/src/main.rs` exposes `--auth-required` and
  `SWIFTPIPE_AUTH_REQUIRED=1`, refusing startup with exit code 78 unless a
  token is configured.
- `repo/crates/swift-api/tests/auth_required_startup.rs` verifies the startup
  guard, and `repo/crates/swift-api/src/auth.rs` tests missing, wrong, correct,
  rotated, and probe-bypass behavior.

## Decision

Use shared bearer-token authentication as the production-default SwiftPipe API
scheme. Local development may leave auth disabled, but production deployments
should set `--auth-required` or `SWIFTPIPE_AUTH_REQUIRED=1` and provide
`SWIFTPIPE_AUTH_TOKEN` through a secret manager.

The scheme is intentionally deployment-level rather than tenant-level. It
protects the service boundary while preserving a migration path to richer
identity once multi-tenant authorization requirements are explicit.

## Consequences

- Operators can deploy SwiftPipe behind common ingress, proxy, and secret-store
  tooling without mTLS infrastructure.
- Token rotation is possible by configuring multiple comma-separated tokens
  during the overlap window.
- Bearer tokens must be protected as secrets and transmitted only over TLS in
  production.
- Auth does not currently distinguish tenants, roles, or object prefixes.
- Migrating to per-tenant keys or mTLS will require an explicit compatibility
  plan for existing clients and OpenAPI security metadata.

## Alternatives Considered

- mTLS. Deferred because it adds certificate lifecycle and ingress complexity
  before SwiftPipe has multi-node production requirements.
- Per-tenant API keys. Deferred because tenant identity, authorization scopes,
  and per-tenant rate limits are not yet modeled.
- OAuth/OIDC. Deferred because SwiftPipe currently has no interactive users or
  identity-provider integration requirements.
- No production auth. Rejected because write endpoints can ingest data and
  produce durable artifacts.
