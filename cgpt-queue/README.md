# cgpt-queue — swiftpipe hardening

Autonomous work queue for the GPT 5.5 agent. Each `NNN-*.md` file is one
self-contained hardening task on swiftpipe. The queue is intentionally
sized for long unattended runs — work straight through it.

## Rules of engagement (autonomy-first)

1. **Do not block on human feedback.** If a task touches a detail you'd
   normally clarify with a maintainer, make the best judgment call,
   write it under a `Decision:` line in the task file (or, if it has
   wider implications, in `.context/current-state.md` under "Known
   risks"), and ship the task. Never leave a task half-done.

2. **No `Blocked:` lines unless physically impossible to proceed.** "I'm
   not sure which option the user prefers" is not blocked — pick the
   conservative option and document the choice. The only legitimate
   blocker is a missing artifact you cannot create (e.g. an upstream
   licensed file) — in that case write a `Cannot-proceed:` line with
   exactly what's missing, and move to the next task.

3. **Tasks are independent unless `Depends on:` says otherwise.** Pick
   the lowest-numbered unblocked task and go.

4. **Read project context first.** Start with `AGENTS.md`, `CLAUDE.md`,
   `.context/current-state.md`, `.context/invariants.md`,
   `docs/index.md`, and (inside the repo) `repo/README.md`. Mind the
   project invariants — preserve schema-as-control-surface, keep diffs
   small, don't rewrite architecture.

5. **Run the relevant tests before declaring done.** Per-crate is fine:
   `cargo test -p swift-core`, `cargo test -p swift-schema`,
   `cargo test -p swift-db`, `cargo test -p swift-duckdb`,
   `cargo test -p swift-cli`, `cargo test -p swift-api`. The whole
   workspace test (`cargo test`) is also acceptable. Also run
   `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings`.

6. **Completion = deleting the task file.** Do not "mark done" inline.
   The empty slot is the signal.

7. **Found-bugs go on a `Found:` line in the task file** (or in
   `.context/current-state.md` under "Known risks"). Fix it only if
   the task already calls for it; otherwise note and proceed.

8. **Commit in small chunks.** One task = one or a few commits. Follow
   the existing commit message style in `git log`.

9. **Update `.context/current-state.md`** whenever a material capability
   ships (a crate gains tests, a runbook lands, an opt-in flips to
   default, etc.).

10. **Quality bar.** Each completed task should leave the repo
    measurably more credible — tests that actually run, code that
    typechecks, docs that reflect reality.

## What you may decide on your own

These would normally be Claude-only, but for this queue you are
authorized to make the call inline (document under `Decision:`):

- Choice of lint level (`warn` vs `deny`) for a new clippy group.
- Naming conventions for new modules, env vars, or metric labels — pick
  one consistent with existing names.
- Default values for new tunables (timeouts, sizes, retry counts) —
  pick something operationally sensible and note the reasoning.
- Layout of new tests / fixtures / runbooks.
- Whether a new dep is justified — prefer stdlib or workspace-existing
  crates; if you must add one, prefer well-known maintained crates and
  pin a major version.
- Snapshot format for golden files (prefer human-diffable: CSV, JSON,
  text — not opaque binary).
- Error variant shape and codes returned to the API caller.
- Whether to introduce a new trait vs extend an existing one — prefer
  extending; introduce a trait only when there's a second implementor in
  the same task.

## Out of scope

Almost nothing. The only things reserved for Claude are:

- Replacing the schema-driven control surface with a different model
  (that is the project's defining invariant and requires an ADR).
- Switching the object-store URI scheme from `s3://` to anything else.
- Changing the DuckDB-as-bridge approach for Postgres system-of-record.

Everything else — observability, error taxonomies, hardening,
test scaffolding, docs, CI surface, schema corpus, runbooks — is yours.

## Conventions for the task files

Each file has:

- `Severity:` MINOR / SIGNIFICANT / SAFETY-CRITICAL
- `Depends on:` (optional)
- `Context:` why this matters
- `Scope:` what to do
- `Acceptance:` how to know it's done
- `Notes:` (optional) gotchas

You may add `Decision:`, `Found:`, or (only as last resort)
`Cannot-proceed:` lines as you work.

## Stopping condition

Stop when the queue is empty, or when you have a clear cascade of
`Cannot-proceed:` files where the missing artifact is the same one
(don't loop). At stop time, summarize: tasks completed, tasks that
recorded a `Decision:`, anything genuinely stuck and why.
