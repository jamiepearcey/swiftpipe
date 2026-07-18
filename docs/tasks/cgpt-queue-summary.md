# cgpt-queue Completion Summary

## Scope

This summary covers the completed autonomous hardening queue from tasks
309-355. The implementation repo at `repo/` is not a Git repository in this
workspace, so this is reconciled against the generated queue catalog,
validated files, and `.context/current-state.md` rather than `git log`.

## Completed Tasks

Total completed in this queue segment: 47.

- cicd: 9 tasks, covering release planning, CI cache/concurrency/permissions,
  badges, OSV scanning, actionlint, stable/beta Rust checks, and rustdoc.
- ops: 15 tasks, covering Docker, Helm, Postgres, schema authoring, spec
  reproduction, benchmarking, incident response, backup/restore, disaster
  recovery, log shipping, Compose, observability, Kubernetes probes, and
  Kubernetes resources.
- docs: 10 tasks, adding ADR-0002 through ADR-0011.
- ui: 6 tasks, adding strict indexed TypeScript checks, ESLint, Vitest smoke
  tests, CSP, runtime npm audit, and UI build artifact upload.
- meta: 7 tasks, refreshing current state, invariants, task docs,
  architecture overview, docs index, and this summary.

## Notable Decisions

- UI CI uses Node 22 and a dedicated `UI Build` job.
- ESLint uses flat config with recommended JavaScript, TypeScript, React Hooks,
  and React Refresh rules; `react-hooks/set-state-in-effect` is disabled for
  the initial baseline to avoid changing existing state-reset behavior.
- Vitest uses jsdom and Testing Library with fetch/localStorage stubs so the
  dashboard smoke test does not require a running API.
- UI CSP is same-origin only for scripts, styles, and connections; the lone
  inline style was moved to CSS so `style-src 'unsafe-inline'` is unnecessary.
- `npm audit --omit=dev` is the CI gate for runtime UI dependencies; dev-tool
  Vite/esbuild findings remain tracked separately.
- The current-state refresh preserved the chronological ledger and replaced
  stale gap text with explicit `Note:` entries.
- Invariants were grounded in accepted ADRs and observable behavior.
- Docs task files now point humans at `cgpt-queue/` as the live work queue.

## Cannot-proceed

None.

## Bugs And Risks Found

- Full UI dev-tool auditing still reports 2 moderate Vite/esbuild findings;
  runtime-only `npm audit --omit=dev` reports 0 vulnerabilities. Tracked in
  `.context/current-state.md` and `docs/tasks/backlog.md`.
- A prior `swift-schema` layout inference Criterion run passed but reported a
  statistically significant median slowdown. Tracked in
  `.context/current-state.md`.
- Task 334 found stale docs describing DuckDB as the Postgres bridge; docs now
  record the implemented SQLx Postgres sink.
- Task 339 found stale OpenAPI auth environment wording; `openapi.json` now
  uses `SWIFTPIPE_AUTH_TOKEN`.
