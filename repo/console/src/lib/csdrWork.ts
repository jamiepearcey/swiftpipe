// CSDR penalty-recon WORKFLOW logic (client-side). The read-model snapshot is a
// read-only reconciliation; this layer turns it into the operational work the
// desk actually does before the CSD dispute deadline: join each recon line to
// its accrual, derive a break reason, compute the netting position, model the
// dispute window, and hold an ephemeral disposition worksheet.
//
// Dispositions are a SESSION WORKSHEET only — persisted to localStorage, not to
// the server. The shape here (state + note, keyed by period + line) is the
// prototype for a real persisted dispute lifecycle (see ADR-0012 follow-ons).
import { useSyncExternalStore } from "react";
import type { CsdrAccrual, CsdrReconLine, CsdrSnapshot } from "@/lib/types";

export type BreakReason =
  | "matched"
  | "rate_mismatch"
  | "amount_mismatch"
  | "day_count"
  | "missing_computed"
  | "missing_reported"
  | "other";

/** Ephemeral triage state a user assigns to a break while working the queue. */
export type Disposition = "open" | "investigating" | "accepted" | "flagged";

export interface WorkLine {
  key: string;
  line: CsdrReconLine;
  accrual?: CsdrAccrual;
  reason: BreakReason;
  reasonDetail: string;
  absDiff: number;
  disposition: Disposition;
  note?: string;
}

export const REASON_LABEL: Record<BreakReason, string> = {
  matched: "matched",
  rate_mismatch: "rate mismatch",
  amount_mismatch: "amount mismatch",
  day_count: "day count",
  missing_computed: "missing computed",
  missing_reported: "missing reported",
  other: "unclassified",
};

export function lineKey(l: Pick<CsdrReconLine, "transactionRef" | "isin" | "penaltyType">): string {
  return `${l.transactionRef}|${l.isin}|${l.penaltyType}`.toUpperCase();
}

/** Derive a break reason + short diagnosis. A triage aid over rate/amount/day
 *  arithmetic — not an authoritative determination. */
export function classifyBreak(line: CsdrReconLine, accrual?: CsdrAccrual): { reason: BreakReason; detail: string } {
  if (line.status === "matched") return { reason: "matched", detail: "computed = reported" };
  if (line.status === "missing_computed")
    return { reason: "missing_computed", detail: "CSD billed a penalty we did not expect — no independent check, investigate first" };
  if (line.status === "missing_reported")
    return { reason: "missing_reported", detail: "we computed a penalty the CSD did not bill — monitor, no action" };

  // status === "break": both sides present, non-trivial diff.
  if (accrual && accrual.referenceAmount > 0 && accrual.penaltyRateBps > 0) {
    const impliedBps = (line.reported / accrual.referenceAmount) * 10000;
    const ourBps = accrual.penaltyRateBps;
    if (Math.abs(impliedBps - ourBps) / ourBps > 0.05) {
      return {
        reason: "rate_mismatch",
        detail: `rate ${ourBps} vs ${impliedBps.toFixed(2)} bps implied — instrument-type classification differs`,
      };
    }
    const oneDaySefp = (ourBps / 10000) * accrual.referenceAmount;
    if (oneDaySefp > 0 && Math.abs(Math.abs(line.diff) - oneDaySefp) / oneDaySefp < 0.2) {
      return { reason: "day_count", detail: "≈ one day of SEFP — ISD / resolution-date disagreement" };
    }
    return { reason: "amount_mismatch", detail: "reference amount / FX source difference" };
  }
  return { reason: "other", detail: "no matching accrual to diagnose" };
}

/** Join recon lines to their accruals and attach reason + disposition. */
export function buildWorkLines(
  snapshot: CsdrSnapshot,
  dispositions: Record<string, DispositionEntry>,
): WorkLine[] {
  const byKey = new Map<string, CsdrAccrual>();
  for (const a of snapshot.accruals) byKey.set(lineKey(a), a);
  return snapshot.lines.map((line) => {
    const key = lineKey(line);
    const accrual = byKey.get(key);
    const { reason, detail } = classifyBreak(line, accrual);
    const d = dispositions[key];
    return {
      key,
      line,
      accrual,
      reason,
      reasonDetail: detail,
      absDiff: Math.abs(line.diff),
      disposition: d?.disposition ?? "open",
      note: d?.note,
    };
  });
}

export const isBreak = (w: WorkLine): boolean => w.line.status !== "matched";

// ---------------------------------------------------------------------------
// Netting position — the treasury question: what do we owe / are owed per
// counterparty per currency, and how does it change if open disputes succeed.
// ---------------------------------------------------------------------------
export interface NettingCell {
  counterparty: string;
  currency: string;
  computed: number;
  reported: number;
  /** Reported, but with lines flagged for dispute swapped to our computed
   *  figure (i.e. the position if every flagged dispute is upheld). */
  disputeAdjusted: number;
  lines: number;
  openBreaks: number;
}

export function netting(work: WorkLine[]): NettingCell[] {
  const cells = new Map<string, NettingCell>();
  for (const w of work) {
    const counterparty = w.line.counterparty ?? "—";
    const currency = w.line.currency || "—";
    const k = `${counterparty}|${currency}`;
    const cell =
      cells.get(k) ??
      { counterparty, currency, computed: 0, reported: 0, disputeAdjusted: 0, lines: 0, openBreaks: 0 };
    cell.computed += w.line.computed;
    cell.reported += w.line.reported;
    cell.disputeAdjusted += w.disposition === "flagged" ? w.line.computed : w.line.reported;
    cell.lines += 1;
    if (isBreak(w) && (w.disposition === "open" || w.disposition === "investigating")) cell.openBreaks += 1;
    cells.set(k, cell);
  }
  return [...cells.values()].sort((a, b) => b.reported - a.reported);
}

// ---------------------------------------------------------------------------
// Dispute window. CSD statement lands ~4th business day of the month after the
// penalty period; the dispute window is ~10 business days from the statement.
// Holidays are NOT modelled (weekends only) — a starter approximation.
// ---------------------------------------------------------------------------
const STATEMENT_BUSINESS_DAY = 4;
const DISPUTE_WINDOW_BUSINESS_DAYS = 10;

export interface DisputeWindow {
  period: string;
  statementDate: Date;
  cutoff: Date;
  businessDay: number; // BD elapsed since statement (1-based); 0 = statement day
  businessDaysRemaining: number;
  state: "upcoming" | "open" | "closing" | "closed";
}

/** Latest YYYY-MM across the accruals' intended settlement dates. */
export function derivePeriod(snapshot: CsdrSnapshot): string | null {
  let latest: string | null = null;
  for (const a of snapshot.accruals) {
    const m = a.intendedSettlementDate?.slice(0, 7);
    if (m && (!latest || m > latest)) latest = m;
  }
  return latest;
}

function isWeekday(d: Date): boolean {
  const day = d.getUTCDay();
  return day !== 0 && day !== 6;
}

function addBusinessDays(from: Date, n: number): Date {
  const d = new Date(from);
  let added = 0;
  while (added < n) {
    d.setUTCDate(d.getUTCDate() + 1);
    if (isWeekday(d)) added += 1;
  }
  return d;
}

function businessDaysBetween(from: Date, to: Date): number {
  if (to <= from) return 0;
  const d = new Date(from);
  let count = 0;
  while (d < to) {
    d.setUTCDate(d.getUTCDate() + 1);
    if (isWeekday(d)) count += 1;
  }
  return count;
}

export function disputeWindow(period: string, now = new Date()): DisputeWindow {
  const [y, m] = period.split("-").map(Number);
  // First day of the month AFTER the penalty period, then the Nth business day.
  const firstOfNext = new Date(Date.UTC(y, m, 1)); // m is 1-based → Date month m == next month
  const statementDate = addBusinessDays(new Date(firstOfNext.getTime() - 86_400_000), STATEMENT_BUSINESS_DAY);
  const cutoff = addBusinessDays(statementDate, DISPUTE_WINDOW_BUSINESS_DAYS);

  let state: DisputeWindow["state"];
  let businessDaysRemaining = 0;
  let businessDay = 0;
  if (now < statementDate) {
    state = "upcoming";
    businessDaysRemaining = DISPUTE_WINDOW_BUSINESS_DAYS;
  } else if (now > cutoff) {
    state = "closed";
  } else {
    businessDaysRemaining = businessDaysBetween(now, cutoff);
    businessDay = DISPUTE_WINDOW_BUSINESS_DAYS - businessDaysRemaining;
    state = businessDaysRemaining <= 3 ? "closing" : "open";
  }
  return { period, statementDate, cutoff, businessDay, businessDaysRemaining, state };
}

// ---------------------------------------------------------------------------
// Disposition worksheet — ephemeral, localStorage-backed, per (period, line).
// ---------------------------------------------------------------------------
export interface DispositionEntry {
  disposition: Disposition;
  note?: string;
  at: number;
}
type Worksheet = Record<string, Record<string, DispositionEntry>>; // period → key → entry

const KEY = "swiftpipe:csdr:worksheet";
let sheet: Worksheet = load();
const subs = new Set<() => void>();

function load(): Worksheet {
  try {
    return JSON.parse(localStorage.getItem(KEY) ?? "{}") as Worksheet;
  } catch {
    return {};
  }
}
function persist() {
  try {
    localStorage.setItem(KEY, JSON.stringify(sheet));
  } catch {
    /* private mode */
  }
  subs.forEach((s) => s());
}

export function setDisposition(period: string, key: string, disposition: Disposition, note?: string): void {
  const month = { ...(sheet[period] ?? {}) };
  if (disposition === "open" && !note) delete month[key];
  else month[key] = { disposition, note, at: Date.now() };
  sheet = { ...sheet, [period]: month };
  persist();
}

export function clearWorksheet(period: string): void {
  sheet = { ...sheet, [period]: {} };
  persist();
}

export function useWorksheet(period: string | null): Record<string, DispositionEntry> {
  return useSyncExternalStore(
    (cb) => {
      subs.add(cb);
      return () => subs.delete(cb);
    },
    () => (period ? sheet[period] ?? EMPTY : EMPTY),
  );
}
const EMPTY: Record<string, DispositionEntry> = {};

// ---------------------------------------------------------------------------
// CSV export — the recon certificate: every line with its disposition.
// ---------------------------------------------------------------------------
export function toCsv(work: WorkLine[]): string {
  const head = [
    "transaction_ref",
    "isin",
    "counterparty",
    "currency",
    "penalty_type",
    "computed",
    "reported",
    "diff",
    "status",
    "reason",
    "disposition",
    "note",
  ];
  const rows = work.map((w) =>
    [
      w.line.transactionRef,
      w.line.isin,
      w.line.counterparty ?? "",
      w.line.currency,
      w.line.penaltyType,
      w.line.computed,
      w.line.reported,
      w.line.diff,
      w.line.status,
      w.reason,
      w.disposition,
      (w.note ?? "").replace(/[\n,]/g, " "),
    ].join(","),
  );
  return [head.join(","), ...rows].join("\n");
}

export function downloadCsv(filename: string, csv: string): void {
  const blob = new Blob([csv], { type: "text/csv" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  a.click();
  URL.revokeObjectURL(url);
}
