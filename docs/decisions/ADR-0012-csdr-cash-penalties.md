# ADR-0012 — CSDR cash-penalty domain, engine, and read-model

## Status

Accepted (starter). 2026-07-19.

## Context

CSDR (Central Securities Depositories Regulation) Settlement Discipline imposes
**cash penalties** on settlement fails. MT537 *Statement of Pending Transactions*
arrives from the market carrying the failing/pending transactions (ISIN,
quantity, posting amount `:19A::PSTA`, intended settlement date `:98A::SETT`,
counterparty `:95P::PSET`, status `:25D::IPRC//PEND`). Each **calendar month**
the CSD issues a penalty statement; a firm must reconcile its **own expected**
penalties against the **CSD-reported** figures and manage the resulting breaks —
the same recon shape as the existing MT940 cash reconciliation.

The existing `ingest-pnl` gives Tier-1 cash P&L over normalized events. There is
no penalty domain yet, and `SecurityEvent` dropped the `:19A:` posting amount
(the penalty base) and the currency.

## Decision

1. **New pure crate `ingest-penalty`** (mirrors `ingest-pnl`: deps `ingest-core`
   + `serde` only, no I/O). It owns the CSDR domain:
   - `InstrumentType` + a **configurable `PenaltyRateTable`** with documented
     *starter* default rates (bps) from the Annex of Commission Delegated
     Regulation (EU) 2017/389 — **not certified coverage** (same discipline as
     the starter schemas).
   - `PenaltyType` (SEFP / LMFP; starter computes SEFP only).
   - `PenaltyAccrual` — an expected penalty computed from a failing/pending
     securities event: `rate_bps/10_000 × reference_amount × business_days_failed`.
   - `ReportedPenalty` — a line from the CSD monthly statement (+ a lenient CSV
     parser, `parse_penalty_statement_csv`).
   - `reconcile_penalties(computed, reported)` → `Vec<PenaltyReconLine>` +
     `PenaltyReconSummary` (matched / break / missing-reported / missing-computed,
     totals, net, break amount), keyed on (transaction ref, ISIN, penalty type).

2. **Extend `SecurityEvent`** additively with `amount: Option<f64>` and
   `currency: Option<String>` (the posting amount is the penalty base; a holding
   leaves them `None`). Populated by `swift-normalize` and `ingest-tabular`.

3. **Data plane** — `ingest-parquet` gains `penalty_accruals.parquet` and
   `penalty_statements.parquet` writers.

4. **Read-model** — the `ingest serve` Axum server gains `GET /csdr/snapshot`
   (a `CsdrSnapshot { version, summary, accruals[], breaks[] }`, camelCase),
   built by querying the two penalty Parquet tables via DuckDB and running the
   pure `reconcile_penalties` — exactly mirroring `/recon/snapshot`. `ingest
   store` writes the penalty tables (and imports a monthly statement CSV via
   `--penalty-statement`).

5. **UIs** — both the swiftpipe console and the quant/pricing UI surface a CSDR
   penalty-recon section, consuming `/csdr/snapshot` through a proxy lane cloned
   from the existing `/recon` lane. swiftpipe owns the engine and data; the
   pricing UI is a pure consumer (its `recon` module already proxies to `:7390`).

## Starter limitations (tracked, not certified)

- Rate table values are representative defaults; real use needs the certified
  Annex rates + MiFID II liquidity classification per ISIN.
- `business_days_failed` defaults to 1 (single-day accrual); real accrual counts
  business days from ISD to settlement/as-of, per currency calendar.
- Only SEFP is computed; LMFP (late-matching, retroactive to ISD) is deferred.
- Currency is best-effort (the `:19A:` currency prefix is stripped at the
  `materialize` layer); a store-level default currency fills the gap.
- Instrument classification is a heuristic over ISIN/description.

## Consequences

Additive to the `ingest-core` contract (ADR-0001) and the read-model; no rewrite.
Golden snapshots and the `SecurityEvent` round-trip tests are updated. The penalty
engine is pure and unit- + fixture-tested against the real MT537 sample.
