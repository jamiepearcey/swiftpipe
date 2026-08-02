// Pure reconciliation derivations over a ReconSnapshot. Mirrors
// CashStatement::reconciles() (opening + Σ signed entries == closing, cent
// tolerance) and the Tier-1 cash P&L (ingest-pnl) client-side, so Books views
// share one computation. Unit-testable, no React.
import type { ReconSnapshot, SnapshotStatement } from "@/lib/types";

export interface StatementVerdict {
  net: number;
  expectedClosing: number;
  diff: number;
  reconciles: boolean;
}

export function verdict(s: SnapshotStatement): StatementVerdict {
  const net = s.entries.reduce((a, e) => a + e.signedAmount, 0);
  const expectedClosing = s.opening + net;
  const diff = s.closing - expectedClosing;
  return { net, expectedClosing, diff, reconciles: Math.abs(diff) < 0.005 };
}

export interface CashPnl {
  inflows: number;
  outflows: number;
  net: number;
  byType: { type: string; amount: number }[];
  statements: number;
  entries: number;
  allReconciled: boolean;
  breaks: number;
}

export function cashPnl(snap: ReconSnapshot): CashPnl {
  let inflows = 0;
  let outflows = 0;
  let entries = 0;
  let breaks = 0;
  const byType = new Map<string, number>();
  for (const s of snap.statements) {
    if (!verdict(s).reconciles) breaks++;
    for (const e of s.entries) {
      entries++;
      if (e.signedAmount >= 0) inflows += e.signedAmount;
      else outflows += -e.signedAmount;
      const t = e.transactionType || "—";
      byType.set(t, (byType.get(t) ?? 0) + e.signedAmount);
    }
  }
  return {
    inflows,
    outflows,
    net: inflows - outflows,
    byType: [...byType.entries()]
      .map(([type, amount]) => ({ type, amount }))
      .sort((a, b) => Math.abs(b.amount) - Math.abs(a.amount)),
    statements: snap.statements.length,
    entries,
    allReconciled: breaks === 0,
    breaks,
  };
}

/** Positions grouped by safekeeping account, quantities summed per ISIN. */
export function positionsByAccount(snap: ReconSnapshot) {
  const groups = new Map<string, ReconSnapshot["positions"]>();
  for (const p of snap.positions) {
    const acct = p.safekeepingAccount || "—";
    const arr = groups.get(acct) ?? [];
    arr.push(p);
    groups.set(acct, arr);
  }
  return [...groups.entries()].map(([account, positions]) => ({ account, positions }));
}
