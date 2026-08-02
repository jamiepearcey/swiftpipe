# ADR-0013 — Anchored sequences: one schema-driven parser for every MT type

## Status

Accepted. 2026-07-20.

## Context

`swift-api`'s job pipeline is the schema-driven engine: `swift-core` parses FIN
structure, `swift-schema` matches a YAML schema, `swift-db` materializes rows,
and the result renders back to FIN for a round-trip check. This is the
**one** parsing protocol for every MT type — 61 securities-settlement schemas
(MT3xx/5xx) already use it uniformly, with no per-message-type Rust code.

MT940 (Customer Statement Message, SWIFT Category 9) had no schema, so
uploading one to `swift-api` failed outright
(`"failed to materialize <id> (MT940)"`) — the console's Parser "Validate on
server" button, which offers this for any message, silently could not serve
cash statements.

The tempting fix — special-case MT940 in `swift-api` to route through the
existing bespoke `swift_mt940::parse_mt940` Rust parser instead — was
rejected: it reintroduces the exact two-pipeline split this project is meant
to avoid, just moved into a different crate. **The actual gap: MT940 is still
a FIN block message with `:NN:` tag lines, structurally no different from
MT535/537 — it doesn't have a schema, not because it can't be schema-described,
but because nobody had extended the schema DSL to express its shape.**

That shape is genuinely different, though. MT940's Statement Line group
(`:61:` + an optional `:86:`) **repeats with no `:16R:`/`:16S:` wrapper** —
unlike every message type this repo's schemas describe so far. `swift-core`'s
`sequence_path` computation is hardcoded to nest only on literal `16R`/`16S`
tags, so without a real fix, the schema-driven pipeline could not correlate a
`:61:` line with its own `:86:` narrative at all.

## Decision

Extend the shared parsing engine, additively, to support this real SWIFT
structural pattern — confirmed against the authoritative SWIFT Category 9
field/sequence specification (Sequence A General Information, Sequence B
Statement Line — repetitive, Sequence C closing) and cross-checked against
the already-proven `swift_mt940::parse_mt940` grouping logic (`:61:` starts an
entry, `:86:` attaches to the immediately preceding one, anything else closes
it):

1. **`swift-core`**: a new `AnchoredSequence` config (`name`, `anchor_tag`,
   `member_tags`) and `parse_message_with_sequences` — the anchor tag starts
   (or restarts) an occurrence, declared member tags stay inside it, any other
   tag closes it. Root-scope only. `parse_message`/`parse_message_with_limits`
   keep their exact existing signatures and behavior, delegating with an
   empty list — every one of the 61 existing 16R/16S-based schemas is
   byte-for-byte unaffected.
2. **`swift-schema`**: `SequenceSchema` gains `anchor_tag`/`member_tags`
   (additive, `#[serde(default)]`), and `anchored_sequences(schema)` extracts
   them for the caller to pass into `parse_message_with_sequences`. Every
   downstream mechanism — `match_message`, `validate_sequences`,
   `materialize_with_schema`'s row-grouping by `(entity, sequence_path)` — is
   already generic over `SequenceFrame` and needed **no changes**: an anchored
   occurrence groups fields into one row exactly like a 16R/16S occurrence
   does. The one real gap: `render_block4` unconditionally emitted
   `:16R:`/`:16S:` wrapper lines for every sequence; anchored sequences have
   no such wrapper in the wire format, so emitting one would corrupt the
   round-trip render. Fixed by checking `anchor_tag.is_some()` before emitting.
3. **`examples/schemas/mt940.yaml`**: a real schema (Sequence A/B/C, `ENTRY`
   declared as `anchor_tag: "61", member_tags: ["86"]`), field formats
   transcribed from the authoritative SWIFT Category 9 reference. Verified
   byte-exact round-trip against `examples/mt940_sample.fin` through the full
   `swift-api` job pipeline (upload → materialize → hydrate → export →
   render), and through `swift-duckdb`'s stricter parse→materialize→DuckDB→
   render→re-parse→re-validate harness (all 62 schemas, MT940 included).
4. **`swift-api`'s `process_input`**: content-sniffs the message (unconditional,
   ahead of any caller-supplied `message_type` hint — a `:61:` line never
   collides with a securities-message tag) and, only when the resolved
   schema declares an anchored sequence, re-parses with
   `parse_message_with_sequences` before the **unchanged** materialize/
   render/hydrate pipeline. For every other schema this is a no-op re-parse.
5. **UHB spec cross-check**: `examples/.uhb/finmt940.md` — iso20022.org's UHB
   catalogue only covers the ISO 15022 "generic field" family (MT5xx etc.);
   it 404s for MT940, which predates that convention. The cached file is
   manually transcribed (not scraped) from the authoritative SWIFT Category 9
   field formats, in the same machine-parseable table shape the repo's own
   `uhb_spec_parser` requires, with its real provenance stated at the top.
6. **Console fix**: `Parser.tsx` stripped the `MT` prefix before sending
   `message_type` to `swift-api`, but the schema catalog is keyed with the
   prefix (`"MT535"`, `"MT940"`) and `swift-api` does no normalization — the
   "Validate on server" button silently mismatched for **every** message type
   whenever a type was inferred, not just MT940. Fixed by sending it verbatim.

## Consequences

- MT940 (and, mechanically, MT942/MT950 — the same Category 9 shape) is now a
  schema-driven message type like any other: one parser, one materialize
  path, one render path, no special-cased Rust branch in `swift-api`.
- `SecurityEvent`/CSDR/recon (ADR-0001, ADR-0012) are unaffected — those
  consume `swift_mt940`'s normalized `CashStatement` output directly via the
  `ingest` CLI's separate data-plane pipeline, which is a different, already-
  correct concern (columnar Parquet for the recon/CSDR read-model) from
  "can a user paste this into the Parser and validate it."
- Full workspace `cargo test` green: 52 test groups, 0 failures, including
  the MT940 golden snapshot, the UHB spec-reproduction suite, and the
  swift-duckdb full-corpus round-trip (all 62 schemas).
