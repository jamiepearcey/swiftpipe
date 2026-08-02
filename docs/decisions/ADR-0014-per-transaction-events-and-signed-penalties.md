# ADR-0014 — Per-transaction normalized events and direction-signed penalties

## Status

Accepted. 2026-08-02. Amends [ADR-0012](ADR-0012-csdr-cash-penalties.md).

## Context

A review of the CSDR penalty work landed in ADR-0012 found five defects that all
traced back to two modelling gaps rather than to coding slips.

**The event was the message.** `swift-normalize::normalize` emitted one
`SecurityEvent` per SWIFT message and scanned the whole `ParsedOutputBatch` for
each field, taking the first match. MT537 *Statement of Pending Transactions* is
a list — a real statement carries many failing transactions. Verified: an MT537
with two pending transactions (GBP 5,000,000 gilt + GBP 2,000,000 bond) produced
one accrual worth 50.00; the second fail was silently dropped. Three further
defects followed from the same batch-wide first-match scan:

- `settlement_date` fell back to any column containing `date`, so it returned the
  `:98A::STAT//` statement date instead of the `:98A::SETT//` intended settlement
  date. ISD is the legal basis for CSDR accrual, so this was a wrong value, not a
  missing one. Every fixture reported `2026-05-11` for a message stating
  `2026-05-12`.
- `currency` found no column (materialize strips the ISO prefix from `:19A:`
  amounts) and a store-level default filled in, labelling a GBP penalty as EUR.
- A multi-message batch would have given every event the first message's data.
  Latent — only the CLI called `normalize`, one message per batch.

There was also no per-transaction reference in the model at all. `PenaltyAccrual`
keyed recon on `message_id`, and the CLI substituted the input **filename stem**
so keys would not collide. The sample CSD statement was written to match those
filenames. A real CSD statement keys on the transaction reference — `:20C::SEME`
and `:20C::RELA` are both present in the messages and were both discarded.

**The sign lived outside the number.** `direction` (`payable` / `receivable`) was
parsed, persisted to Parquet, and never read by `reconcile_penalties`. A
receivable was summed as though it were a payable, inflating `reported_total` and
`break_amount`, and a computed payable matched against a reported receivable of
equal magnitude netted to zero and reported as *matched*.

The enabling fact for the fix: `NormalizedRow.values` already carries
`message_id` and `sequence_path` (e.g. `STAT[0]/TRAN[1]/TRANSDET[0]`), and the
un-normalized capture survives in the `{column}__render_{name}` payload column.
Nothing new had to be threaded through `swift-db` — the data was there and
`swift-normalize` was ignoring it.

## Decision

1. **One normalized event per transaction, not per message.** `normalize`
   restricts rows to the owning `message_id`, then emits one `SecurityEvent` per
   ISIN-bearing row, using that row's `sequence_path` as the transaction scope.
   Field lookups prefer rows in the same branch (ancestor, descendant, or sibling
   under the same immediate parent — MT537's `LINK` block is a sibling of
   `TRANSDET` under `TRAN`), nearest first, falling back to root scope for
   message-level context. A message with no ISIN-bearing row keeps the previous
   one-event-per-message behavior, so no existing message type regresses.

2. **`SecurityEvent` gains `transaction_ref: Option<String>`** — the reference a
   CSD penalty statement keys on (`:20C::RELA`, else `:20C::SEME`). `message_id`
   keeps identifying the message. `ingest-penalty` keys accruals on
   `transaction_ref`, falling back to `message_id`. The CLI's filename-stem
   substitution is deleted.

3. **Dates and currency are selected by qualifier, not by column name.**
   Settlement date is read from the parsed fields by qualifier `SETT` on a `98x`
   tag, scoped to the transaction branch, and never falls back to a
   statement-level date — `None` beats a wrong date. Currency is recovered from
   the ISO 4217 prefix on the amount's render payload column.

4. **Penalty amounts are direction-signed**: payable negative (the account owner
   owes the CSD), receivable positive. Applied at construction in both
   `compute_penalty_accruals` and `parse_penalty_statement_csv`, so accrual,
   reported, recon-line and summary figures are consistent by construction.
   `break_amount` stays a non-negative magnitude. A direction mismatch is now a
   break rather than a match that nets to zero.

   The sign lives *in the amount* rather than in a parallel `direction` string
   precisely because the parallel-field design is what allowed the original bug:
   a field that must be consulted to interpret a number will eventually not be
   consulted.

5. **`CsdrSnapshot::VERSION` → 2**, since (4) changes the meaning of every amount
   in a payload two UIs consume. The swiftpipe console gates on the version and
   refuses to render an unexpected one rather than displaying v1 magnitudes as
   v2 signed values.

6. **`PenaltyType` is an enum** (`Sefp` / `Lmfp`), as ADR-0012 specified and the
   first implementation did not. The struct fields stay `String` at the wire
   boundary so the Parquet column and JSON value remain `"SEFP"`.

7. Supporting fixes: the reported-statement CSV is parsed with a real CSV reader
   (quoted fields containing commas previously shifted every later column);
   `business_days_failed` is persisted to Parquet instead of being hardcoded to
   `1` on read-back; `classify_instrument` drops its unused `isin` parameter.

## Consequences

Additive to the `ingest-core` contract (ADR-0001) — `SecurityEvent` gains a
field, `positions.parquet` and `penalty_accruals.parquet` each gain a column.
Breaking at the `/csdr/snapshot` boundary, which is why the version tag exists;
**the external quant/pricing UI consumes this endpoint and needs the same sign
handling.** Test count 456 → 466.

ADR-0012's starter limitations are narrowed: currency is now read from the
message rather than defaulted, and recon keys on a real transaction reference.
The remaining limitations stand — representative rather than certified rate
table, `business_days_failed` still defaults to 1, SEFP only, heuristic
instrument classification.

A residual asymmetry is worth naming: within one snapshot, `computed` /
`reported` / `diff` are signed while `referenceAmount` and `penaltyRateBps` are
not. Anything comparing a reported amount against a rate must use the magnitude.
Nothing in the JSON says so.
