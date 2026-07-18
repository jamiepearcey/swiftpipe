# ADR-0011: Default Job Render Validation To On

## Status

Accepted

## Context

SwiftPipe jobs can emit rendered FIN messages as durable artifacts. Those
artifacts should be parseable and structurally consistent by default, because
downstream operators may use them for replay, reconciliation, or evidence during
incident response.

The implemented API defaults render validation on:

- `repo/crates/swift-api/src/manifest.rs` models `render_validate` as an
  optional job-request field.
- `repo/crates/swift-api/src/job.rs` uses `request.render_validate.unwrap_or(true)`
  for job requests and sets upload jobs to `render_validate: true`.
- `repo/crates/swift-api/src/openapi.json` documents `render_validate` with
  `"default": true`.
- `repo/crates/swift-api/src/main.rs` validates the cached schema catalog for
  rendering at startup.
- `repo/crates/swift-cli/tests/render_cli.rs` and
  `repo/crates/swift-schema/tests/roundtrip.rs` cover render validation and
  parse-render-reparse behavior.

## Decision

Default `render_validate` to on for API job requests and uploads.

Callers may explicitly set `render_validate: false` when they need a diagnostic
or experimental render attempt, but doing so opts out of the default downstream
parseability guarantee for rendered artifacts.

## Consequences

- Normal job outputs fail fast when rendered FIN cannot be validated.
- Rendered artifacts are safer to hand to downstream replay and reconciliation
  workflows.
- Schema and render metadata defects surface during job processing rather than
  being hidden until a later consumer parses the artifact.
- Jobs can take additional CPU time for render validation.
- Disabling validation is an explicit caller choice and should be treated as a
  weaker artifact-quality mode in operational reviews.

## Alternatives Considered

- Default validation off for throughput. Rejected because rendered artifacts are
  durable outputs and parseability is more important than saving validation work
  by default.
- Validate only in CLI workflows. Rejected because API-created artifacts need
  the same default guarantees as local render commands.
- Make validation mandatory with no opt-out. Rejected because diagnostic
  workflows may need to inspect imperfect render output while schema work is in
  progress.
