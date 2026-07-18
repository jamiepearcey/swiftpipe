# Prompt for GPT 5.5 — swiftpipe hardening (autonomous)

You have a work queue at `finance/swiftpipe/cgpt-queue/`. Each
`NNN-*.md` file is one self-contained hardening task. Read
`cgpt-queue/README.md` first — it sets the autonomy rules.

## Your job

1. Pick the lowest-numbered unblocked task. Respect `Depends on:`.
2. Read the task file end to end. Then read project context:
   `finance/swiftpipe/AGENTS.md`, `finance/swiftpipe/CLAUDE.md`,
   `finance/swiftpipe/.context/current-state.md`,
   `finance/swiftpipe/.context/invariants.md`,
   `finance/swiftpipe/docs/index.md`, and
   `finance/swiftpipe/repo/README.md`.
3. Execute the task. Honor project rules: minimum-diff, preserve
   architecture, no scope creep beyond the task file.
4. Run the relevant tests (per-crate `cargo test -p <crate>` or the
   workspace `cargo test`). Also `cargo fmt --check` and
   `cargo clippy --workspace --all-targets -- -D warnings`. If you add
   tests, they must pass.
5. **Delete the task file when fully done.** Deletion = completion.
6. Commit in small reviewable chunks. Match the existing commit-message
   style.

## Autonomy — the load-bearing rule

**Do not stop to ask the user anything.** This queue is designed to
run unattended. When a task underspecifies something (a name, a
default, a layout, an error code, a snapshot format), pick the
conservative option, record it on a `Decision:` line in the task
file, and ship it. The README enumerates what you are authorized to
decide on your own — that list is intentionally broad.

The only legitimate stopper is a missing external artifact you
physically cannot create (a licensed UHB page, an upstream binary, a
remote credential). In that case write a `Cannot-proceed: <thing>`
line and skip to the next task. Never use `Blocked:` — that pattern
is from other queues; here it just means "I gave up too easily."

## Out of scope

Reserved for Claude only:

- Replacing the schema-driven control surface with a different model.
- Switching the `s3://` object URI scheme.
- Changing the DuckDB-bridge approach for the Postgres
  system-of-record.

Everything else — observability, error taxonomies, hardening,
test scaffolding, docs, CI surface, schema-corpus tests, runbooks,
ADRs that ratify already-shipped decisions — is yours to land.

## Quality bar

Each completed task should leave the repo measurably more credible
than you found it — tests that actually run, code that actually
typechecks, docs that reflect reality. Don't mark a task done if it
doesn't.

## When the queue empties

Stop. Summarize:

- Tasks completed (count).
- Tasks that recorded a `Decision:` and which decisions were
  notable.
- Anything written as `Cannot-proceed:` and what artifact would
  unblock it.
- Bugs you found and where they're tracked (`Found:` lines or
  `.context/current-state.md`).
