#!/usr/bin/env python3
"""Generate the swiftpipe cgpt-queue.

Run from anywhere:
    python3 finance/swiftpipe/cgpt-queue/_generate.py

This is idempotent: it overwrites every NNN-*.md file it owns.
It does NOT touch README.md, PROMPT.md, or this script.
"""
from __future__ import annotations

import os
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO_ROOT = HERE.parent  # finance/swiftpipe
SCHEMAS_DIR = REPO_ROOT / "repo" / "examples" / "schemas"


def task(
    seq: int,
    slug: str,
    title: str,
    severity: str,
    context: str,
    scope: str,
    acceptance: str,
    notes: str = "",
    depends: str = "",
) -> tuple[str, str]:
    """Return (filename, contents)."""
    filename = f"{seq:03d}-{slug}.md"
    body = f"# {seq:03d} — {title}\n\n"
    body += f"**Severity:** {severity}\n\n"
    if depends:
        body += f"Depends on: {depends}\n\n"
    body += f"## Context\n\n{context.strip()}\n\n"
    body += f"## Scope\n\n{scope.strip()}\n\n"
    body += f"## Acceptance\n\n{acceptance.strip()}\n"
    if notes:
        body += f"\n## Notes\n\n{notes.strip()}\n"
    return filename, body


def list_mts() -> list[str]:
    if not SCHEMAS_DIR.exists():
        print(f"WARN: {SCHEMAS_DIR} not found; using empty MT list", file=sys.stderr)
        return []
    out = []
    for entry in sorted(SCHEMAS_DIR.iterdir()):
        if entry.suffix == ".yaml" and entry.stem.startswith("mt"):
            out.append(entry.stem)  # e.g. "mt540"
    return out


def main() -> int:
    mts = list_mts()
    tasks: list[tuple[str, str]] = []
    seq = 1

    def add(slug, title, severity, context, scope, acceptance, notes="", depends=""):
        nonlocal seq
        tasks.append(task(seq, slug, title, severity, context, scope, acceptance, notes, depends))
        seq += 1

    # ─────────────────────────────────────────────────────────────────────
    # SECTION A — code hygiene & lint baseline (per crate, fine-grained)
    # ─────────────────────────────────────────────────────────────────────
    crates = [
        ("swift-core", "Zero-copy structural FIN parser. No async, no I/O."),
        ("swift-schema", "Schema loading, validation, render metadata."),
        ("swift-db", "DB-agnostic materialization sink and inferred layout."),
        ("swift-duckdb", "DuckDB store + Parquet export."),
        ("swift-cli", "CLI binary `swiftpipe-cli`."),
        ("swift-api", "Self-hosted axum API."),
    ]
    for crate, blurb in crates:
        add(
            f"hygiene-deny-warnings-{crate}",
            f"Add `#![deny(warnings, rust_2018_idioms, unsafe_code, missing_debug_implementations)]` to {crate}",
            "MINOR",
            f"{crate}: {blurb}. The crate currently has no top-level deny attributes, so latent issues slip through.",
            f"In `crates/{crate}/src/lib.rs` (or `main.rs` for binaries), add the deny attributes at the top. Fix any new warnings that surface — do not suppress them with `allow` unless there's a clear inline reason. For binaries, `unsafe_code` must remain forbidden.",
            "`cargo clippy -p {crate} --all-targets -- -D warnings` passes. `cargo test -p {crate}` still passes.".replace("{crate}", crate),
            "If a warning surfaces in third-party-derived code, narrow the `allow` to the specific item, not the whole module.",
        )
    for crate, _ in crates:
        add(
            f"hygiene-msrv-{crate}",
            f"Pin `rust-version` in {crate}/Cargo.toml",
            "MINOR",
            f"{crate} does not declare a minimum supported Rust version. CI uses `stable`, so we need a recorded floor.",
            f"Edit `crates/{crate}/Cargo.toml`. Add `rust-version = \"1.82\"` (matches the Dockerfile's `rust:1.82`). Run `cargo check -p {crate}` to confirm.",
            "rust-version is set and `cargo check -p {crate}` succeeds.".replace("{crate}", crate),
        )
    # Single MSRV CI job (depends on the per-crate pins)
    add(
        "ci-msrv-job",
        "Add an MSRV CI job that checks against pinned Rust version",
        "MINOR",
        "MSRV is now declared per crate (tasks 007–012). Add a CI job that verifies the workspace builds against that floor — otherwise the field rots silently.",
        "In `repo/.github/workflows/ci.yml`, add a job `msrv` that uses `dtolnay/rust-toolchain@1.82` and runs `cargo check --workspace --all-targets`. Cache via `Swatinem/rust-cache@v2` with `shared-key: msrv`.",
        "New CI job is present and structured like the existing `check` job. `actionlint` (if available) reports no errors on the workflow file.",
        depends="007,008,009,010,011,012",
    )
    # Per-crate clippy pedantic
    for crate, _ in crates:
        add(
            f"hygiene-clippy-pedantic-{crate}",
            f"Turn on `clippy::pedantic` group in {crate} (with curated allows)",
            "MINOR",
            f"{crate} currently runs only default clippy. Pedantic catches a useful set of bugs and stylistic regressions if curated.",
            f"In `crates/{crate}/src/lib.rs` (or `main.rs`) add `#![warn(clippy::pedantic)]`. Then `#![allow(...)]` the unhelpful subset: at minimum `clippy::module_name_repetitions, clippy::must_use_candidate, clippy::missing_errors_doc, clippy::missing_panics_doc, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap, clippy::similar_names`. Fix anything else pedantic reports. Decision: prefer fixing the code over expanding the allow-list, but if a fix would harm clarity, add one inline `#[allow]` at the call-site with a one-line reason.",
            "`cargo clippy -p {crate} --all-targets -- -D warnings` passes. `cargo test -p {crate}` still passes.".replace("{crate}", crate),
        )

    # Drop unwrap/expect from non-test code in specific files
    unwrap_targets = [
        ("swift-api/src/main.rs", "swift-api main"),
        ("swift-api/src/job.rs", "swift-api job"),
        ("swift-api/src/job_store.rs", "swift-api job_store"),
        ("swift-api/src/queue.rs", "swift-api queue"),
        ("swift-api/src/object_store.rs", "swift-api object_store"),
        ("swift-api/src/routes.rs", "swift-api routes"),
        ("swift-api/src/system_record.rs", "swift-api system_record"),
        ("swift-api/src/auth.rs", "swift-api auth"),
        ("swift-cli/src/main.rs", "swift-cli main"),
        ("swift-duckdb/src/lib.rs", "swift-duckdb lib"),
        ("swift-schema/src/lib.rs", "swift-schema lib"),
        ("swift-core/src/lib.rs", "swift-core lib"),
        ("swift-db/src/lib.rs", "swift-db lib"),
    ]
    for path, label in unwrap_targets:
        add(
            f"hygiene-unwrap-{path.replace('/', '-').replace('.rs', '')}",
            f"Audit and remove non-test `.unwrap()` / `.expect()` in {label}",
            "SIGNIFICANT",
            f"Production `.unwrap()` / `.expect()` panics in `crates/{path}` produce uninformative crash-loops in operations. Replace them with returned errors (or, where genuinely infallible after construction, document why with a short comment + keep `.expect(\"<invariant>\")`).",
            f"Open `crates/{path}`. For each non-test `.unwrap()` / `.expect()`: (a) if the operation is genuinely fallible, propagate via `?` with `anyhow::Context` describing what failed; (b) if it's infallible by construction, keep `.expect(\"<invariant>\")` and add a one-line comment explaining the invariant. Do not introduce new error types unless the existing one is wrong — extend `anyhow` contexts. Tests are out of scope.",
            "No bare `.unwrap()` / `.expect()` calls without a paired invariant comment remain outside `#[cfg(test)]` blocks in the file. `cargo test -p <owning crate>` passes.",
            notes="A few main-startup expects (e.g. `Arc::get_mut` in `swift-api/src/main.rs`) are genuinely infallible — keep those with a comment.",
        )

    # Per-crate forbid unsafe
    for crate, _ in crates:
        add(
            f"hygiene-forbid-unsafe-{crate}",
            f"Add `#![forbid(unsafe_code)]` to {crate}",
            "MINOR",
            f"{crate} has no documented need for unsafe. Forbid it explicitly so it can't be reintroduced casually.",
            f"In `crates/{crate}/src/lib.rs` (or `main.rs`), add `#![forbid(unsafe_code)]` (above any other crate attributes). If a legitimate unsafe is later needed, the introducer must demote to `deny` with a recorded reason.",
            "`cargo build -p {crate}` passes; no unsafe is present.".replace("{crate}", crate),
        )

    # rustfmt config + edition2024 readiness
    add(
        "hygiene-rustfmt-config",
        "Add `repo/rustfmt.toml` with explicit edition + width + reorder rules",
        "MINOR",
        "There is no rustfmt.toml. CI runs `cargo fmt --check` but anyone editing offline gets defaults that can drift across rust versions.",
        "Create `repo/rustfmt.toml` with: `edition = \"2021\"`, `max_width = 100`, `reorder_imports = true`, `reorder_modules = true`, `use_field_init_shorthand = true`, `use_try_shorthand = true`. Then run `cargo fmt --all` to normalize the workspace; commit any churn in a single follow-up commit so the substantive change stays reviewable.",
        "rustfmt.toml exists. `cargo fmt --check` is green.",
    )
    add(
        "hygiene-cargo-deny",
        "Add `repo/deny.toml` and a CI `cargo deny check` job",
        "SIGNIFICANT",
        "Supply-chain hygiene currently relies on `cargo audit`. `cargo deny` adds license, source, and duplicate-version policy on top.",
        "Add `repo/deny.toml` with sections: `[advisories]` (yanked + ignore none by default), `[licenses]` (allow MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Unicode-DFS-2016, MPL-2.0; copyleft = deny), `[bans]` (multiple-versions = warn), `[sources]` (unknown-registry = deny). Then add a `deny` job to `repo/.github/workflows/ci.yml` mirroring the `audit` job, installing `cargo-deny --locked` and running `cargo deny check`.",
        "`cargo deny check` passes locally; CI job is added.",
    )
    add(
        "hygiene-cargo-machete",
        "Add `cargo machete` check to CI for unused workspace dependencies",
        "MINOR",
        "Unused deps inflate compile time and supply-chain surface. `cargo machete` is fast and false-positive-rare.",
        "Add a `machete` CI job that installs `cargo-machete --locked` and runs `cargo machete` from `repo/`. Fix any real positives in-place; add `[package.metadata.cargo-machete] ignored = [\"...\"]` only for false positives (e.g. crates loaded by feature).",
        "`cargo machete` clean locally and in CI.",
    )
    add(
        "hygiene-cargo-udeps",
        "Add `cargo udeps` nightly CI job (warn-only)",
        "MINOR",
        "Complementary to machete: udeps runs on nightly and catches deps only used in certain feature combinations.",
        "Add a `udeps` job to `repo/.github/workflows/ci.yml` using `dtolnay/rust-toolchain@nightly`, installing `cargo-udeps --locked`, and running `cargo udeps --workspace --all-targets`. Set `continue-on-error: true` because nightly false-positives happen.",
        "Job is present, warn-only, and runs to completion against nightly.",
    )
    add(
        "hygiene-cargo-sort",
        "Sort dependencies in all Cargo.toml files",
        "MINOR",
        "Some Cargo.toml files have ad-hoc dep ordering, making diffs noisier.",
        "Install `cargo install cargo-sort --locked`. Run `cargo sort --workspace --check`; if it complains, run `cargo sort --workspace` and commit. Add a `cargo-sort` step to the CI `check` job (install + `--check`).",
        "`cargo sort --workspace --check` exits 0 locally and in CI.",
    )

    # ─────────────────────────────────────────────────────────────────────
    # SECTION B — schema-corpus hardening (per MT)
    # ─────────────────────────────────────────────────────────────────────
    # Per-MT round-trip test
    for mt in mts:
        add(
            f"schema-roundtrip-{mt}",
            f"Add a parse→render→reparse round-trip test for {mt.upper()}",
            "SIGNIFICANT",
            f"`examples/{mt}_sample.fin` exists and `examples/schemas/{mt}.yaml` exists, but there is no per-MT regression test asserting that parse → render → reparse is byte-stable (or byte-stable modulo a documented normalization set). Without it, schema edits silently break round-trips.",
            f"In `crates/swift-schema/tests/`, add a test file (or extend an existing fixture-driven test) that:\n1. Loads the catalog from `examples/schemas/`.\n2. Parses `examples/{mt}_sample.fin`.\n3. Renders it back via `render_message`.\n4. Re-parses the rendered output and asserts the field-slice set is equal (compare structurally; rendered byte equality is preferable when achievable, but for MTs where SWIFT permits multiple equivalent surface forms, normalize to a canonical structural form first and document the normalization in a short test-comment).\nDecision: when normalization is needed, prefer comparing `(tag, qualifier, value)` tuples per sequence path; record the choice in the test comment.",
            f"`cargo test -p swift-schema` runs the new round-trip test for {mt.upper()} and it passes.",
            "If the round-trip materially fails (not just a whitespace nit), add a `Found:` line in this task naming the field and the failure mode before deleting the task.",
        )

    # Per-MT golden parquet snapshot
    for mt in mts:
        add(
            f"schema-golden-{mt}",
            f"Add a golden normalized-output snapshot test for {mt.upper()}",
            "SIGNIFICANT",
            f"Regression-detection on materialized output currently relies on the aggregate `all5` demo. Per-MT goldens catch schema edits that break exactly one message family.",
            f"In `crates/swift-schema/tests/goldens/` (create if absent), commit a small text-form snapshot of the normalized field rows produced by parsing `examples/{mt}_sample.fin` against the canonical schema. Add a test that regenerates and diffs against the snapshot. Decision: snapshot format = newline-delimited JSON, one record per row, keys sorted alphabetically — this diffs better than YAML/CSV and is trivial to regenerate.",
            f"`cargo test -p swift-schema` runs the new golden test for {mt.upper()}. The snapshot file is committed under `crates/swift-schema/tests/goldens/{mt}.ndjson`.",
            "Provide an `UPDATE_GOLDENS=1 cargo test` escape hatch in the test driver so legitimate schema updates can regenerate snapshots in one shot.",
        )

    # Per-MT schema coverage assertion
    for mt in mts:
        add(
            f"schema-coverage-{mt}",
            f"Assert non-empty render metadata for {mt.upper()} schema",
            "MINOR",
            f"`schema render-validate` and `schema coverage` exist as CLI commands but there's no per-MT unit test asserting that {mt.upper()}'s render mappings cover all its inbound fields.",
            f"Extend (or add) a `crates/swift-schema/tests/render_metadata.rs` test that loads only the `{mt}.yaml` schema and asserts: (a) `coverage` reports zero `missing_render_qualifiers` for fields appearing in `examples/{mt}_sample.fin`; (b) zero `ambiguous_render_options`. If either is non-zero, the test should fail with a structured message listing the offending fields. Fix the schema (not the test) when failures are real coverage gaps.",
            f"Test exists, fails meaningfully on regression, and currently passes for {mt.upper()}.",
        )

    # ─────────────────────────────────────────────────────────────────────
    # SECTION C — parser hardening (swift-core)
    # ─────────────────────────────────────────────────────────────────────
    parser_tasks = [
        ("max-message-size", "Enforce a configurable max message size in `parse_message`",
         "Currently `parse_message` accepts arbitrary `&[u8]`. A multi-GB hostile payload would exhaust memory before any structural validation triggers.",
         "Introduce `parse_message_with_limits(input, limits: &ParseLimits)`. `ParseLimits` has at minimum: `max_total_bytes: usize` (default 10 MiB), `max_block_bytes: usize` (default 4 MiB), `max_fields: usize` (default 10_000), `max_sequence_depth: u32` (default 16). When a limit is exceeded, return a new `ParseDiagnostic::LimitExceeded { kind, limit, observed }`. Existing `parse_message` calls the new function with `ParseLimits::default_strict()` so existing callers get the protection automatically. Decision: defaults are strict but overridable so back-compatible callers can opt out via `ParseLimits::lenient()`."),
        ("malformed-block-fuzz",  "Add a `cargo fuzz` target for `parse_message` malformed-block coverage",
         "Structural parser is the front door; fuzz-coverage gaps here turn into DoS bugs.",
         "Install `cargo install cargo-fuzz --locked` workflow. Add `crates/swift-core/fuzz/` with a fuzz target `parse_message_fuzz` that calls `parse_message(data)` and asserts no panic. Add a small handcrafted corpus under `crates/swift-core/fuzz/corpus/parse_message_fuzz/` derived from a few `examples/*.fin` files. Add a documented `cargo fuzz run parse_message_fuzz -- -max_total_time=60` smoke command in `docs/workflows/testing.md`."),
        ("malformed-tag-fuzz", "Add a `parse_text_fields` fuzz target focused on malformed tags",
         "The field-tag scanner is a hotspot for off-by-one bugs. Targeted fuzz coverage there is cheap.",
         "Inside `crates/swift-core/fuzz/`, add a second target `parse_text_fields_fuzz` that wraps the input in a synthetic `{4:...-}` block and calls `parse_message`. Seed it with corpora of pure tag-line edge cases (no value, multi-line, embedded `:`)."),
        ("proptest-roundtrip", "Add a proptest that random valid messages parse without diagnostics",
         "Round-trip random generation against the structural grammar catches bugs invisible to corpus tests.",
         "Add `proptest = \"1\"` as a `[dev-dependencies]` entry. In `crates/swift-core/tests/proptest_roundtrip.rs`, define a strategy that emits structurally valid (block, sequence, field) trees and asserts `parse_message` returns no diagnostics and reproduces the original tree structurally. 256 cases at default seed."),
        ("zero-copy-bench", "Add a Criterion bench asserting the parser stays zero-copy on a large input",
         "Performance regressions in the parser show up as allocator pressure. A repeatable bench catches them.",
         "Extend `crates/swift-core/benches/parse_message.rs` with a benchmark `parse_500kb` that parses a synthesized 500 KiB message. Document the expected throughput baseline (≥ 400 MiB/s on a 2024 MBP, per `repo/README.md`) in a header comment."),
        ("offset-tracking", "Verify byte offsets in `ParseDiagnostic` variants are message-relative",
         "Some diagnostics carry `offset` fields but it's not obvious whether they're block-relative or message-relative. Operators reading errors deserve a documented contract.",
         "Add a `crates/swift-core/tests/diagnostic_offsets.rs` test that feeds known-bad messages (each block at known offsets) and asserts the reported offset equals the byte index in the original input. Where the implementation currently reports block-relative offsets, fix it to report message-relative; update doc-comments on each variant accordingly."),
    ]
    for slug, title, ctx, scope in parser_tasks:
        add(
            f"parser-{slug}",
            title,
            "SIGNIFICANT",
            ctx,
            scope,
            "`cargo test -p swift-core` passes. Where the task adds a bench or fuzz target, its smoke invocation runs to completion.",
        )

    # ─────────────────────────────────────────────────────────────────────
    # SECTION D — API surface hardening (swift-api)
    # ─────────────────────────────────────────────────────────────────────
    endpoints = ["upload", "jobs_create", "jobs_list", "job_status", "manifest", "object", "metrics", "healthz", "readyz", "openapi"]
    for ep in endpoints:
        add(
            f"api-test-{ep}",
            f"Add a focused integration test for the `{ep}` endpoint",
            "SIGNIFICANT",
            f"`swift-api` currently lacks per-endpoint integration tests beyond happy-path. Each endpoint needs its own test covering the success path AND at least two error paths.",
            f"In `crates/swift-api/tests/{ep}_endpoint.rs` (create if needed), spin up the router with a tempdir-backed `LocalObjectStore` and assert:\n- The happy path returns the documented status (e.g. 200 for GET, 202 for queued POST, 200 for sync POST when queue is disabled).\n- At least one bad-request case (malformed JSON, oversize body, missing input selection, etc.) returns the documented error code.\n- At least one not-found case where applicable.\nReuse helpers in existing `crates/swift-cli/tests/render_cli.rs` style. Decision: use `axum::Router::oneshot` for direct request injection — no need to bind a real port.",
            f"`cargo test -p swift-api --test {ep}_endpoint` runs at least 3 cases and they pass.",
        )

    # API per-endpoint hardening details
    api_hardening = [
        ("timeout-middleware", "Add a per-request timeout middleware to `/v1/*`",
         "Long-running handlers (parquet generation, large prefix jobs) can pin worker threads indefinitely if the client hangs.",
         "Wire `tower::timeout::TimeoutLayer` into `make_router` for the `/v1/*` routes with a configurable timeout (CLI flag `--request-timeout-secs`, env `SWIFTPIPE_REQUEST_TIMEOUT_SECS`, default 120). Translate `tower::timeout::error::Elapsed` to HTTP 504 via the `ApiError` mapping. Async submissions (`upload`/`jobs` when queue is enabled) should be exempt — they return 202 quickly.",
         "Test that a synthetic slow handler hits 504 after the configured timeout."),
        ("rate-limit-token-bucket", "Add a per-IP token-bucket rate limit on `/v1/upload` and `/v1/jobs`",
         "Unauthenticated dev mode (no `SWIFTPIPE_AUTH_TOKEN`) is exposed to abuse if anyone reaches the port. A modest in-memory rate limit makes that safer.",
         "Implement an in-process token-bucket keyed by `X-Forwarded-For` (fallback: socket peer). CLI flags: `--rate-limit-rps` (default 5), `--rate-limit-burst` (default 20). Apply only to write endpoints. Return 429 with a `Retry-After` header on exhaustion. Use `governor` crate (well-maintained) — pin a major version.",
         "Test injects 50 requests in a tight loop and asserts at least one 429."),
        ("idempotency-key", "Honor `Idempotency-Key` on `/v1/upload`",
         "Clients retrying on transient network errors today double-submit work. A header-keyed idempotency table fixes that.",
         "Add an in-memory `LruCache<String, JobId>` keyed by the `Idempotency-Key` header (TTL 24h, capacity 10k). On a hit, return the previously-issued `job_id` + 202 instead of submitting a new task. Decision: per-process only — clustered deployments can layer a redis backend later behind the same interface.",
         "Test posts the same payload twice with the same header and asserts identical job_id."),
        ("structured-error-codes", "Document and stabilize API error codes via an enum",
         "`map_job_error` currently dispatches on substring matches against `anyhow` messages — fragile and undocumented.",
         "Add `crates/swift-api/src/error.rs` defining `enum ApiErrorCode { BadRequest, NotFound, PayloadTooLarge, ServiceUnavailable, Timeout, RateLimited, Unauthorized, Internal }` with a stable `as_str()` mapping. Replace the substring dispatch with a typed error returned from the `job::process_*` functions (introduce a `JobError` enum in `crates/swift-api/src/job.rs`). Update `openapi.json` to document the codes.",
         "Round-trip test asserts each variant maps to the documented HTTP status + code string."),
        ("max-prefix-fanout", "Cap maximum prefix-job fanout per request",
         "A prefix job pointed at a bucket with 1M+ objects will blow out memory before any processing happens.",
         "Add CLI flag `--max-prefix-fanout` (default 10_000). In `process_job_request`, after listing matches, return `bad_request` with a typed error if the count exceeds the cap, suggesting `--include-suffix` filtering or smaller prefixes.",
         "Test creates a synthetic 100-object fanout and asserts a cap of 50 returns 400."),
        ("auth-token-rotate", "Document & test multi-token rotation in `AuthLayer`",
         "Operations cannot rotate `SWIFTPIPE_AUTH_TOKEN` without a brief outage because only a single token is accepted.",
         "Extend `AuthLayer` to accept a comma-separated list of valid tokens (still in `SWIFTPIPE_AUTH_TOKEN`; values containing commas are not supported and that is documented). Comparison stays constant-time per token, short-circuits at first match.",
         "Test asserts two tokens both pass; a third does not."),
        ("queue-depth-metric", "Export `swiftpipe_queue_depth` as a real gauge",
         "`metrics_prometheus_handler` currently reports `queue_depth = in_flight` — that's mis-labeled. Real queue depth is the number of pending tasks not yet picked up by a worker.",
         "Expose `JobQueue::len()` (the sender's `capacity() - permits`) via `AppState.metrics.queue_depth: AtomicI64`, updated on submit/dispatch. Update both Prometheus and JSON metrics handlers.",
         "Test fills the queue beyond worker capacity and asserts metric > 0."),
        ("job-duration-histogram", "Add `swiftpipe_job_duration_seconds` Prometheus histogram",
         "Operators currently get only counters. A duration histogram is the single most useful SLO signal for ingestion latency.",
         "Add `prometheus = \"0.13\"` as a dep, register a histogram with buckets `[0.05, 0.1, 0.25, 0.5, 1, 2.5, 5, 10, 30, 60, 120]`, and observe per-job total duration on completion. Replace the bespoke text-formatting in `metrics_prometheus_handler` with `prometheus::TextEncoder` output for ALL metrics (move the existing counters into the registry).",
         "`curl /metrics` returns valid Prometheus format including the histogram series."),
        ("graceful-shutdown-test", "Add a graceful-shutdown integration test",
         "`main.rs` installs SIGTERM/Ctrl+C handlers but there's no test that in-flight jobs complete before the process exits.",
         "Add `crates/swift-api/tests/graceful_shutdown.rs` that starts the server in-process, submits N jobs, sends a shutdown signal via the same channel `main.rs` uses, and asserts the JoinSet drains within 30s and all submitted jobs reach a terminal state.",
         "Test passes."),
        ("readyz-checks-postgres", "When system-of-record=postgres, `/readyz` returns 503 until the pool is live",
         "Today `/readyz` only checks paths; if Postgres is misconfigured the API will accept jobs that immediately fail.",
         "Add a `ReadyCheck` on `AppState` that the `postgres` adapter populates with a probe future (`SELECT 1`). `readyz` awaits it (with a 1s timeout) and returns 503 + reason on failure.",
         "Integration test with a deliberately bad connection string asserts 503."),
        ("backpressure-503", "Document and test the bounded-queue 503 contract",
         "Returning 503 when the queue is full is already implemented; there's no test pinning the behavior. Operators rely on the 503 to scale.",
         "Add `crates/swift-api/tests/queue_full.rs` that constructs a queue with capacity 1, fills it with a slow synthetic task, and asserts subsequent POSTs return 503 with the documented error body.",
         "Test passes."),
        ("manifest-strict-schema", "Validate `JobRequest` strictly with `serde(deny_unknown_fields)`",
         "Today the JSON request silently ignores unknown keys, so typos like `out_prefix` are lost.",
         "Add `#[serde(deny_unknown_fields)]` to `JobRequest` (and any nested structs that are user-faceable). Add a test that an unknown key returns 400 with a clear message.",
         "Test passes."),
        ("openapi-generated-from-types", "Generate `openapi.json` from types via `utoipa` (or hand-validate)",
         "The committed `openapi.json` is hand-edited and drifts from `JobRequest` definitions.",
         "Decision: introduce `utoipa = \"4\"` + `utoipa::ToSchema` on the user-facing request/response structs. Replace the static `OPENAPI_JSON` with one rendered at build time. If `utoipa` requires invasive macro surface, fall back: keep the static file but add a `crates/swift-api/tests/openapi_schema_consistency.rs` test that re-derives field names from `JobRequest` via reflection (`serde_introspect` or manual list) and asserts every documented field exists.",
         "OpenAPI doc stays consistent with `JobRequest` field names automatically."),
        ("request-size-headers", "Strictly enforce `Content-Length` against `max_upload_bytes`",
         "`DefaultBodyLimit::max(max_upload)` rejects oversize, but only after partially buffering. Reject early on `Content-Length` for cleaner error messages.",
         "Add a small extractor that inspects `Content-Length` and returns 413 immediately if it exceeds `max_upload_bytes`. Keep the body limit as defense-in-depth for chunked transfers.",
         "Test asserts an oversize `Content-Length` returns 413 with the documented error body before the body bytes are read."),
    ]
    for slug, title, ctx, scope, accept in api_hardening:
        add(
            f"api-{slug}",
            title,
            "SIGNIFICANT",
            ctx,
            scope,
            f"{accept} `cargo test -p swift-api` passes.",
        )

    # ─────────────────────────────────────────────────────────────────────
    # SECTION E — observability
    # ─────────────────────────────────────────────────────────────────────
    obs_tasks = [
        ("trace-parse-stage", "Add `tracing::info_span` around the `parse_message` call in `process_job`",
         "Per-stage spans are needed to attribute latency in production traces."),
        ("trace-schema-stage", "Add a span around schema matching + materialization", ""),
        ("trace-duckdb-stage", "Add a span around DuckDB writes (record `rows_written`)", ""),
        ("trace-parquet-stage", "Add a span around Parquet export (record `bytes_written`)", ""),
        ("trace-zip-stage", "Add a span around zip creation (record file count + size)", ""),
        ("trace-system-record-stage", "Add a span around system-of-record writes", ""),
        ("trace-correlation-id-propagate", "Propagate `x-request-id` into spans as a field, not just as a response header", ""),
        ("metrics-per-message-type", "Add `swiftpipe_messages_processed_total{message_type=...}` counter", ""),
        ("metrics-parquet-bytes", "Add `swiftpipe_parquet_bytes_written_total` counter", ""),
        ("metrics-object-store-ops", "Add `swiftpipe_object_store_ops_total{op=get|put|list}` counter", ""),
        ("metrics-errors-by-code", "Add `swiftpipe_api_errors_total{code=...}` counter", ""),
        ("log-redact-account-numbers", "Add a redaction helper for account numbers in error logs",
         "SWIFT messages contain real account numbers. Their appearance in `tracing::error!` logs is a compliance risk."),
        ("log-redact-bic", "Add a redaction helper for BIC codes in error logs",
         "BICs are not strictly secret but operators often want them masked in shipped logs."),
        ("structured-job-events", "Replace ad-hoc `tracing::info!` job lifecycle logs with a `JobEvent` struct",
         "Today lifecycle logs are free-form strings; downstream log aggregation can't reliably parse them."),
        ("openmetrics-content-type", "Switch `/metrics` to `application/openmetrics-text; version=1.0.0`",
         "Some scrapers (notably k8s' built-in) prefer OpenMetrics."),
        ("metrics-jobs-by-status", "Add `swiftpipe_jobs_total{status=queued|running|succeeded|failed|stuck}` gauge", ""),
        ("trace-on-error-only", "Default `RUST_LOG` example to `swiftpipe_api=info,tower_http=warn` and surface in the runbook", ""),
        ("metrics-upload-bytes", "Add a histogram for upload body size", ""),
        ("metrics-prefix-fanout", "Add a histogram for prefix-job fanout (object count per job)", ""),
        ("metrics-duckdb-rows", "Add `swiftpipe_duckdb_rows_written_total` counter", ""),
    ]
    for entry in obs_tasks:
        slug = entry[0]
        title = entry[1]
        ctx = entry[2] if len(entry) > 2 and entry[2] else f"Observability hardening: {title.lower()}."
        add(
            f"obs-{slug}",
            title,
            "SIGNIFICANT",
            ctx,
            f"Implement the change in the minimum number of files. If a Prometheus metric, register it via the prometheus registry from `api-job-duration-histogram` (or introduce that registry if that task hasn't landed yet). If a span, use `tracing::info_span!` with structured fields, not `info!(\"...\")`. Add or extend a focused test that verifies the metric/span is emitted (use `tracing_subscriber::fmt::TestWriter` for spans; for Prometheus, scrape `/metrics` in the test and grep the series name).",
            "Test asserts the metric/span/log structure is correct. `cargo test -p swift-api` passes.",
        )

    # ─────────────────────────────────────────────────────────────────────
    # SECTION F — security & supply chain
    # ─────────────────────────────────────────────────────────────────────
    sec_tasks = [
        ("dockerfile-hadolint", "Add a hadolint CI job for the Dockerfile",
         "Dockerfile lint catches common security mistakes (latest tags, missing USER, etc.). Hadolint is fast and well-known.",
         "Add a `hadolint` job in CI that runs `hadolint/hadolint-action@v3.1.0` against `repo/Dockerfile`. Fix any errors it surfaces; document any conscious ignores in `.hadolint.yaml`.",
         "Hadolint passes (or has documented ignores)."),
        ("dockerfile-pin-digest", "Pin Docker base-image digests for reproducible builds",
         "`node:22-bookworm-slim`, `rust:1.82-bookworm`, and `debian:bookworm-slim` are mutable tags. Pin by digest so a malicious republish can't slip in.",
         "Replace each `FROM` tag with `FROM <image>@sha256:<digest>` (resolve current digests via `docker pull` + `docker inspect`). Add a note in `repo/Dockerfile` describing the periodic re-pin process and how to verify a digest. Add `deploy/SECURITY.md` (or extend if exists) with the policy.",
         "All `FROM` lines pin by digest. `docker build .` succeeds locally."),
        ("dockerfile-nonroot-test", "Add a test asserting the runtime container runs as a non-root user",
         "`useradd swiftpipe` is in the Dockerfile but there's no `USER swiftpipe` directive visible — verify both the directive and the test.",
         "Audit the Dockerfile for `USER swiftpipe`. Add a `docker-runtime-user` CI step that runs `docker build -t swiftpipe:check .` then `docker run --rm swiftpipe:check id -u` and asserts the result is `10001`.",
         "CI step exists and the assertion holds."),
        ("scripts-shellcheck", "Run shellcheck over all `scripts/*.sh`",
         "Shell scripts are easy to silently break. Shellcheck is mandatory hygiene.",
         "Add a CI step that runs `shellcheck scripts/*.sh`. Fix any errors. Add a shebang + `set -euo pipefail` to any script missing it.",
         "Shellcheck clean locally and in CI."),
        ("scripts-pinned-tools", "Pin versions of `cargo install` invocations in scripts and CI",
         "Unpinned `cargo install` lets a malicious crate update break the supply chain.",
         "Audit all `cargo install ...` calls; ensure each passes `--locked` and `--version <pinned>`. Document the policy in `repo/SECURITY.md`.",
         "All cargo-install invocations are pinned."),
        ("api-cors-default-deny", "Default CORS to `none`; require explicit allowlist",
         "Today CORS defaults to a permissive empty `CorsLayer`. Combined with optional auth, that's a footgun.",
         "Change `make_cors_layer` so that when `SWIFTPIPE_CORS_ORIGINS` is unset the layer reflects no origin (so cross-origin browser requests are rejected). Document the env var as required for any browser-based UI deployment.",
         "Test asserts an `Origin: foo` request with no `SWIFTPIPE_CORS_ORIGINS` is rejected at the CORS layer."),
        ("api-auth-required-prod-flag", "Add a `--auth-required` flag that refuses to start without a token",
         "Today, missing `SWIFTPIPE_AUTH_TOKEN` silently disables auth. Production deployers need a tripwire.",
         "Add `--auth-required` (env `SWIFTPIPE_AUTH_REQUIRED=1`). When set, `main.rs` fails fast (exit code 78 — config error) if no token is configured. Logs a clear message naming the env var.",
         "Test asserts the binary exits non-zero when the flag is set and no token is provided."),
        ("api-input-uri-scheme-restriction", "Reject `file://`, `http://`, and other non-`s3://` URIs at the API boundary",
         "Defense in depth: even with the parser checks, a layer at the handler boundary makes audits easier.",
         "In `process_job_request`, validate that `input_uri` / `input_prefix` / `output_prefix` all start with `s3://` before any object-store call; return `bad_request` with the documented error code.",
         "Test exercises each non-s3 scheme and asserts 400."),
        ("api-zip-bomb-cap", "Cap zip output size and entry count",
         "`exports.zip` is built from job artifacts; a pathological config could produce a huge or many-entry zip used downstream.",
         "Add per-job caps (default: 10 GiB total, 100k entries) enforced inside `process_job`. On exceed, abort the zip stage with a typed `JobError::ZipLimitExceeded` and a structured log line.",
         "Test asserts the caps trigger."),
        ("schema-yaml-billion-laughs", "Audit `serde_yaml` for billion-laughs / DoS payloads",
         "Some YAML implementations are vulnerable to alias-expansion DoS. Need an explicit test.",
         "Add `crates/swift-schema/tests/yaml_dos.rs` that feeds a small synthetic alias-expansion YAML and asserts parsing either rejects it or completes within 500 ms with bounded memory. If `serde_yaml` is vulnerable, replace with `serde_yml = \"0.0.12\"` (a maintained fork with DoS protection) and update all references.",
         "Test passes."),
        ("ci-pin-actions-by-sha", "Pin GitHub Actions versions by SHA",
         "Floating major tags (e.g. `@v4`) can be force-pushed. Pinning by SHA is the recommended security posture.",
         "Replace every `uses: foo/bar@vN` in `repo/.github/workflows/*.yml` with `uses: foo/bar@<sha>  # vN`. Document the upgrade process (Dependabot or manual) in `repo/SECURITY.md`.",
         "All uses lines pinned; CI is green."),
        ("ci-dependabot-config", "Add a Dependabot config covering cargo, docker, and github-actions",
         "Currently nothing automates dep upgrades.",
         "Create `repo/.github/dependabot.yml` with three ecosystems: `cargo` (workspace root), `docker` (`Dockerfile` directory), `github-actions` (`.github/workflows`). Weekly schedule.",
         "Config validates locally (`yamllint`) and Dependabot accepts it once merged."),
        ("ci-sbom-syft", "Generate a Syft SBOM artifact in CI",
         "SBOMs are increasingly required for downstream supply-chain attestations.",
         "Add a `sbom` CI step that runs `anchore/sbom-action@v0` over `repo/` and uploads the SPDX-JSON artifact. Future image-signing tasks can promote this to an OCI attestation.",
         "Artifact appears in CI run output."),
        ("ci-trivy-image-scan", "Scan the built Docker image with Trivy",
         "Stops new CVEs in the image from going unnoticed.",
         "Extend the `docker` CI job to run `aquasecurity/trivy-action@<sha>` against the built image. Fail on HIGH/CRITICAL with a documented allowlist for known-acceptables in `repo/.trivyignore`.",
         "Job runs; allowlist is empty unless justified inline."),
        ("api-correlation-id-from-client", "Honor incoming `x-request-id` if the client supplies one",
         "Today the correlation-id middleware always generates a fresh UUID, breaking client-side trace stitching.",
         "Update `correlation_id_middleware` to prefer `req.headers().get(\"x-request-id\")` when present and valid UTF-8 ≤ 128 chars; otherwise generate. Echo the chosen id in the response header.",
         "Test asserts client-supplied id is echoed."),
    ]
    for slug, title, ctx, scope, accept in sec_tasks:
        add(
            f"sec-{slug}",
            title,
            "SAFETY-CRITICAL",
            ctx,
            scope,
            accept,
        )

    # ─────────────────────────────────────────────────────────────────────
    # SECTION G — performance
    # ─────────────────────────────────────────────────────────────────────
    perf_tasks = [
        ("bench-swift-schema", "Add a Criterion bench for `infer_database_layout` over the full corpus",
         "Schema-layout inference cost matters for cold-start. No bench exists."),
        ("bench-swift-db", "Add a Criterion bench for `materialize_message` against a representative MT540", ""),
        ("bench-swift-duckdb", "Add a Criterion bench for `DuckDbStore::write_batch` (10k rows)", ""),
        ("bench-swift-api-zip", "Add a Criterion bench for the zip-export stage", ""),
        ("bench-regression-baseline", "Commit baseline bench numbers in `docs/workflows/benchmarking.md`",
         "Today the README cites perf numbers from one machine. Pin them in a versioned file so regressions are detectable."),
        ("bench-ci-quick-pass", "Add a `bench --no-run` CI compile check",
         "Catches benches that stop compiling without paying for a full perf run on every PR."),
        ("perf-allocs-parser", "Audit allocations in the hot parser path",
         "Use `cargo-instruments` (or `dhat-rs` as a portable substitute) to find allocs in `parse_message`. Replace any per-call `Vec::new` that could be a `SmallVec` or reused buffer.",),
        ("perf-allocs-renderer", "Audit allocations in the render path", ""),
        ("perf-duckdb-batch-size", "Make DuckDB write batch size configurable", ""),
        ("perf-prefix-parallelism-cap", "Document and bound prefix-job parallelism",
         "Today prefix jobs process objects in parallel without a documented cap. A pathological prefix can saturate the CPU."),
        ("perf-parquet-rowgroup-tuning", "Expose Parquet row-group size as a CLI flag",
         "Default is rarely optimal; expose it and document recommended ranges."),
        ("perf-stream-zip-output", "Stream zip output instead of buffering in memory",
         "Large jobs currently buffer the zip in RAM before writing. Switch to streaming via `ZipWriter` over a `BufWriter<File>`."),
        ("perf-symbol-table-warmup", "Cache `SchemaCatalog::from_yaml_str` results across requests",
         "Each API request currently reloads schemas from disk. Cache the parsed catalog at startup."),
    ]
    for entry in perf_tasks:
        slug = entry[0]
        title = entry[1]
        ctx = entry[2] if len(entry) > 2 and entry[2] else f"Performance hardening: {title.lower()}."
        add(
            f"perf-{slug}",
            title,
            "SIGNIFICANT",
            ctx,
            "Implement using existing benchmark/profiling infrastructure where possible. For new benches, follow the style of `crates/swift-core/benches/parse_message.rs`. For optimizations, include a before/after measurement in the commit message.",
            "Bench or test passes. Document the result in `docs/workflows/benchmarking.md` for perf measurements.",
        )

    # ─────────────────────────────────────────────────────────────────────
    # SECTION H — CI/CD & release
    # ─────────────────────────────────────────────────────────────────────
    cicd_tasks = [
        ("split-test-job-by-crate", "Split the workspace `cargo test` job into per-crate matrix jobs",
         "Per-crate jobs cache better and pinpoint failures faster than a single 6-crate run."),
        ("coverage-llvm-cov", "Add a `cargo-llvm-cov` coverage CI job uploading to artifacts",
         "Coverage is a leading indicator of test-quality regressions."),
        ("release-workflow", "Add a `release.yml` triggered on `v*` tags that builds and publishes artifacts",
         "Tagged releases should produce reproducible binaries and a Docker image."),
        ("release-changelog-config", "Add a `cliff.toml` (git-cliff) config and a `CHANGELOG.md` skeleton",
         "Changelogs are easy to forget. Generate them from commit messages."),
        ("release-cargo-dist", "Add `cargo dist init` for cross-platform binary releases",
         "axum/cli binaries are useful as standalone artifacts on macOS/Linux."),
        ("ci-cache-key-cargo-lock", "Refine CI cache keys to include `Cargo.lock` hash",
         "Today cache keys are `shared-key`-based; including lock-file hash improves hit rate after dep bumps."),
        ("ci-concurrency-cancel", "Add a `concurrency: { group: ..., cancel-in-progress: true }` block to CI",
         "Prevents stacked PR-push runs from consuming CI minutes."),
        ("ci-permissions-readonly", "Set default `permissions: { contents: read }` on every workflow",
         "Principle of least privilege; today workflows inherit default token scopes."),
        ("ci-status-badges", "Add CI status badges to `repo/README.md`",
         "Cheap signal for contributors."),
        ("ci-osv-scanner", "Add a `google/osv-scanner-action` CI job",
         "Complementary to cargo-audit; covers more advisory sources."),
        ("ci-actionlint", "Add `actionlint` CI step over workflows", ""),
        ("ci-rust-stable-and-beta", "Run the `check` job against both `stable` and `beta`",
         "Catches upcoming-rustc breakage early."),
        ("ci-doc-build", "Add a `cargo doc --workspace --no-deps -D warnings` CI job",
         "Catches broken rustdoc links before merge."),
    ]
    for entry in cicd_tasks:
        slug = entry[0]
        title = entry[1]
        ctx = entry[2] if len(entry) > 2 and entry[2] else f"CI hardening: {title.lower()}."
        add(
            f"cicd-{slug}",
            title,
            "SIGNIFICANT",
            ctx,
            "Implement in `repo/.github/workflows/`. Validate with `actionlint` locally if available.",
            "New/modified workflow passes locally (or is structurally sound) and survives `actionlint`.",
        )

    # ─────────────────────────────────────────────────────────────────────
    # SECTION I — operational runbooks & deployment
    # ─────────────────────────────────────────────────────────────────────
    op_tasks = [
        ("runbook-docker-deploy", "Write `docs/workflows/deploy-docker.md` covering local docker run",
         "README has invocation snippets but no runbook covering volumes, env vars, log shipping, and upgrade procedure."),
        ("runbook-helm-chart", "Add a minimal Helm chart under `repo/deploy/helm/` with values for replica count, resources, image",
         "Kubernetes is the most likely deployment target; a starter chart accelerates trial deployments."),
        ("runbook-postgres-system-of-record", "Write `docs/workflows/postgres-system-of-record.md`",
         "Setup steps for the postgres SoR mode are scattered in the README."),
        ("runbook-schema-authoring", "Write `docs/workflows/schema-authoring.md`",
         "There is no end-to-end guide for adding a new MT schema."),
        ("runbook-spec-reproduction", "Write `docs/workflows/spec-reproduction.md`",
         "`./scripts/reproduce-specs-sequentially.sh` exists but the workflow context is undocumented."),
        ("runbook-benchmarking", "Expand `docs/workflows/benchmarking.md` with interpretation guidance",
         "Today the file (if present) just lists commands. Add what good/bad numbers look like, common regressions, and how to bisect."),
        ("runbook-incident-data-corruption", "Write `docs/workflows/incident-data-corruption.md`",
         "Operators need a checklist when a downstream Parquet looks wrong."),
        ("runbook-incident-rate-limit", "Write `docs/workflows/incident-rate-limit-saturation.md`",
         "Triage steps when `swiftpipe_api_errors_total{code=rate_limited}` spikes."),
        ("runbook-backup-restore", "Write `docs/workflows/backup-restore.md`",
         "Object-store and Postgres backup expectations need to be explicit."),
        ("runbook-disaster-recovery", "Write `docs/workflows/disaster-recovery.md`",
         "Cold-start recovery from a backed-up object store + SoR is non-obvious."),
        ("runbook-log-shipping", "Write `docs/workflows/log-shipping.md` covering JSON-log mode",
         "Today logs are pretty-printed. Document the structured-log toggle and Vector/Fluent Bit examples."),
        ("compose-example", "Add `repo/deploy/docker-compose.yml` running api + postgres + grafana",
         "One-command local stack lowers the barrier to demos and integration tests."),
        ("compose-loki-tempo", "Add a docker-compose overlay that wires Loki + Tempo + Grafana",
         "Visual demonstration of the observability story."),
        ("k8s-pod-liveness-readiness", "Document Kubernetes liveness vs readiness mappings",
         "`/healthz` vs `/readyz` distinction is implementation-only; needs ops-facing doc."),
        ("k8s-pod-resource-recs", "Document recommended pod CPU/memory requests/limits",
         "Lessons from local benchmarking should inform resource recommendations."),
    ]
    for entry in op_tasks:
        slug = entry[0]
        title = entry[1]
        ctx = entry[2] if len(entry) > 2 and entry[2] else f"Operational documentation: {title.lower()}."
        add(
            f"ops-{slug}",
            title,
            "MINOR",
            ctx,
            "Write the document or asset specified. Where examples are shown, validate them by running them. Cross-link from `docs/index.md`.",
            "Document exists, is linked from `docs/index.md`, and any commands shown were actually run successfully.",
        )

    # ─────────────────────────────────────────────────────────────────────
    # SECTION J — ADRs ratifying already-shipped decisions (safe for GPT)
    # ─────────────────────────────────────────────────────────────────────
    adrs = [
        ("adr-object-uri-s3-scheme", "ADR: `s3://` URI scheme for object inputs/outputs",
         "Implemented; no ADR records the rationale. Without one, future contributors may add `file://` or `gs://` ad-hoc."),
        ("adr-duckdb-as-postgres-bridge", "ADR: DuckDB as the Postgres bridge for system-of-record",
         "Decided and implemented; the rationale (single embedded SQL surface, deferred SQL Server) needs to be on record."),
        ("adr-bounded-job-queue", "ADR: bounded mpsc job queue with HTTP 503 backpressure",
         "Decided; record the throughput / latency tradeoffs and why we don't use an external queue (yet)."),
        ("adr-local-object-store-default", "ADR: local-disk-backed object store as the default",
         "Decided; record why we ship vendor-neutral local first and what the upgrade path to real S3 looks like."),
        ("adr-schema-yaml-format", "ADR: YAML as the schema authoring format",
         "Decided; record the alternatives considered (JSON, TOML, custom DSL) and why YAML won."),
        ("adr-render-metadata-coupling", "ADR: render metadata lives in the same schema file as parser metadata",
         "Decided; record the coupling rationale and what would justify a future split."),
        ("adr-auth-bearer-token", "ADR: shared bearer-token auth as the production-default scheme",
         "Decided; document the tradeoffs vs mTLS / per-tenant keys and the migration path."),
        ("adr-tracing-as-observability-substrate", "ADR: `tracing` + Prometheus as the observability substrate",
         "Decided; document why we picked these crates over alternatives (slog, log, metrics-rs)."),
        ("adr-cargo-workspace-layout", "ADR: 6-crate workspace layout (core/schema/db/duckdb/cli/api)",
         "Decided; document the boundary rationale so future PRs respect it."),
        ("adr-render-validation-default-on", "ADR: render-validate defaults to ON for job requests",
         "Decided; document why and what disabling it means for downstream guarantees."),
    ]
    for slug, title, ctx in adrs:
        add(
            f"docs-{slug}",
            title,
            "MINOR",
            ctx,
            "Create `docs/decisions/ADR-NNNN-<slug>.md` following the existing `ADR-0001-project-memory-structure.md` template. Pick the next free ADR number. Sections: Context, Decision, Status (Accepted), Consequences, Alternatives Considered. Reference the code/PRs that implement the decision.",
            "ADR file exists with the documented sections, is linked from `docs/index.md`, and references real code.",
        )

    # ─────────────────────────────────────────────────────────────────────
    # SECTION K — UI (minimal — UI work is normally Sonnet, but tests/lint are GPT-safe)
    # ─────────────────────────────────────────────────────────────────────
    ui_tasks = [
        ("ui-tsc-strict", "Enable `strict` and `noUncheckedIndexedAccess` in `ui/tsconfig.json`",
         "Stricter TS catches a class of runtime errors at build time. Today's tsconfig has not been audited."),
        ("ui-eslint-baseline", "Add an ESLint config and `npm run lint` CI step",
         "There's no lint baseline for the UI; bugs slip through code review."),
        ("ui-vitest-baseline", "Add Vitest + one smoke test asserting the dashboard renders without throwing",
         "No frontend tests today; a smoke test is the minimum to detect bundling-time crashes."),
        ("ui-csp-meta", "Add a Content-Security-Policy meta tag to `ui/index.html`",
         "Defense in depth; the UI loads scripts from a single origin so a strict CSP is feasible."),
        ("ui-npm-audit-ci", "Add an `npm audit --omit=dev` CI step",
         "Catches vulnerable runtime dep updates."),
        ("ui-build-ci", "Add a `ui-build` CI step that runs `npm ci && npm run build` and uploads dist",
         "Today the Rust CI happily green-lights PRs that break the UI build."),
    ]
    for slug, title, ctx in ui_tasks:
        add(
            f"ui-{slug}",
            title,
            "MINOR",
            ctx,
            "Implement in `repo/ui/`. Validate locally with `cd repo/ui && npm ci && npm run build`. CI changes go in `repo/.github/workflows/ci.yml`.",
            "UI build passes; new CI step is green.",
        )

    # ─────────────────────────────────────────────────────────────────────
    # SECTION L — context/state housekeeping (GPT-safe; small, verifiable)
    # ─────────────────────────────────────────────────────────────────────
    ctx_tasks = [
        ("context-current-state-refresh", "Refresh `.context/current-state.md` to reflect the actual current implementation",
         "The file lists `Active gaps` that have been closed (API harness, schema lifecycle) and omits much of what now ships."),
        ("context-invariants-expand", "Add concrete, code-grounded invariants to `.context/invariants.md`",
         "Today the file is mostly framing. Add invariants like `s3://` URI scheme, bounded queue, render-validate default, deny_unknown_fields on user-facing structs."),
        ("docs-tasks-current-refresh", "Refresh `docs/tasks/current.md` with the live cgpt-queue pointer",
         "The current-tasks file does not mention the cgpt-queue; humans reading project memory will miss it."),
        ("docs-tasks-backlog-refresh", "Refresh `docs/tasks/backlog.md` to reflect what has shipped",
         "Backlog items overlap with completed work."),
        ("docs-architecture-system-overview-expand", "Expand `docs/architecture/system-overview.md` with actual subsystem boundaries",
         "Today it's a stub. Replace with crate-boundary diagram + module ownership table."),
        ("docs-index-link-runbooks", "Update `docs/index.md` to link the new runbooks landing under `docs/workflows/`",
         "As runbooks land, the index needs to be the discoverability surface."),
    ]
    for slug, title, ctx in ctx_tasks:
        add(
            f"meta-{slug}",
            title,
            "MINOR",
            ctx,
            "Edit the named file. Cross-link related context. Do not delete content without a one-line `Note:` recording what was superseded.",
            "File is updated and reads coherently against the current implementation.",
        )

    # Final task: completion summary stub
    add(
        "meta-queue-completion-summary",
        "Write a queue-completion summary in `docs/tasks/cgpt-queue-summary.md`",
        "MINOR",
        "When the queue empties, the autonomous run should leave behind a brief audit-grade summary of what changed.",
        "Once this is the only task left (or there are <=3 other tasks remaining), create `docs/tasks/cgpt-queue-summary.md` listing: tasks completed (by section), notable `Decision:` lines made during the run, anything written as `Cannot-proceed:`, and bugs found.",
        "File exists and reads accurately against `git log`.",
    )

    # ─────────────────────────────────────────────────────────────────────
    # Emit
    # ─────────────────────────────────────────────────────────────────────
    # First, clear any prior NNN-*.md files in this directory (idempotency).
    for existing in HERE.iterdir():
        if re.match(r"^\d{3}-.*\.md$", existing.name):
            existing.unlink()

    for filename, body in tasks:
        (HERE / filename).write_text(body)

    print(f"Wrote {len(tasks)} task files to {HERE}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
