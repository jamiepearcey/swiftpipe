import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { cn } from "@/lib/utils";
import { PlaneHeader } from "@/components/PlaneHeader";
import { StatTile } from "@/components/StatTile";
import { DataTable, type Column } from "@/components/DataTable";
import { StatusBadge, type StateVariant } from "@/components/StatusBadge";
import { TagChip, type TagTone } from "@/components/TagChip";
import { NodeErrorBanner } from "@/components/NodeErrorBanner";
import { PillarEmpty } from "@/components/PillarEmpty";
import { Btn, Segmented } from "@/components/setup/kit";
import { getCsdrSnapshot, type CsdrResult } from "@/lib/api";
import { fmtAmount, fmtSigned, relTime } from "@/lib/format";
import type { AppRoute } from "@/lib/route";
import {
  buildWorkLines,
  derivePeriod,
  disputeWindow,
  downloadCsv,
  isBreak,
  lineKey,
  netting,
  REASON_LABEL,
  setDisposition,
  toCsv,
  useWorksheet,
  type BreakReason,
  type Disposition,
  type DisputeWindow,
  type WorkLine,
} from "@/lib/csdrWork";
import {
  ArrowRight,
  CalendarClock,
  Download,
  Eye,
  Flag,
  Check,
  Gavel,
  RefreshCw,
  ShieldAlert,
} from "lucide-react";

type ViewTab = "breaks" | "netting" | "all";

const REASON_TONE: Record<BreakReason, TagTone> = {
  matched: "ok",
  rate_mismatch: "warn",
  amount_mismatch: "warn",
  day_count: "warn",
  missing_computed: "error",
  missing_reported: "muted",
  other: "muted",
};

const STATUS_VARIANT: Record<string, StateVariant> = {
  matched: "ok",
  break: "error",
  missing_computed: "warn",
  missing_reported: "neutral",
};

const DISPO_VARIANT: Record<Disposition, StateVariant> = {
  open: "neutral",
  investigating: "info",
  accepted: "ok",
  flagged: "active",
};

/**
 * Regulatory plane — CSDR settlement-penalty **work queue**. The monthly cycle:
 * MT537 fails accrue an expected penalty; the CSD's statement is reconciled
 * against it; breaks are triaged and disputed before the CSD's ~10-business-day
 * dispute cutoff. This screen owns that work — exception-first queue, derived
 * break diagnosis, netting, and an ephemeral disposition worksheet (localStorage,
 * not persisted server-side). Starter framework — rates/dates not certified.
 */
export function Csdr({
  route,
  onRoute,
}: {
  route: Extract<AppRoute, { view: "csdr" }>;
  onRoute: (r: AppRoute) => void;
}) {
  const [result, setResult] = useState<CsdrResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [at, setAt] = useState(0);
  const live = useRef(true);

  const load = useCallback(async () => {
    try {
      const r = await getCsdrSnapshot();
      if (live.current) {
        setResult(r);
        setError(null);
        setAt(Date.now());
      }
    } catch (e) {
      if (live.current) setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    live.current = true;
    load();
    const t = setInterval(load, 10_000);
    return () => {
      live.current = false;
      clearInterval(t);
    };
  }, [load]);

  const snapshot = result?.snapshot ?? null;
  const period = useMemo(() => (snapshot ? derivePeriod(snapshot) : null), [snapshot]);
  const worksheet = useWorksheet(period);
  const window = useMemo(() => (period ? disputeWindow(period) : null), [period]);

  const work = useMemo(
    () => (snapshot ? buildWorkLines(snapshot, worksheet) : []),
    [snapshot, worksheet],
  );
  const breaks = useMemo(() => work.filter(isBreak), [work]);
  const openBreaks = useMemo(
    () => breaks.filter((w) => w.disposition === "open" || w.disposition === "investigating"),
    [breaks],
  );
  const unresolved = openBreaks.reduce((a, w) => a + w.absDiff, 0);
  const cells = useMemo(() => netting(work), [work]);

  const [tab, setTab] = useState<ViewTab>("breaks");
  const selected = route.txnRef ? work.find((w) => lineKey(w.line).startsWith(route.txnRef!.toUpperCase())) : undefined;

  if (!snapshot && error) {
    return (
      <Shell onRefresh={load}>
        <NodeErrorBanner error={error} />
        <PillarEmpty
          icon={ShieldAlert}
          title="CSDR read-model unreachable"
          body="Run `ingest serve` against a store with penalty accruals + a monthly statement to work the recon."
        />
      </Shell>
    );
  }
  if (!snapshot) {
    return (
      <Shell onRefresh={load}>
        <div className="text-[12.5px] text-muted-foreground">Loading CSDR snapshot…</div>
      </Shell>
    );
  }

  const s = snapshot.summary;
  const dispositioned = breaks.filter((w) => w.disposition !== "open").length;

  return (
    <Shell
      onRefresh={load}
      actions={
        <Btn
          onClick={() => downloadCsv(`csdr-recon-${period ?? "current"}.csv`, toCsv(work))}
          className="inline-flex items-center gap-1.5"
        >
          <Download className="h-3.5 w-3.5" /> Export
        </Btn>
      }
    >
      {result?.stale && (
        <div className="rounded-lg border border-warn/45 bg-warn/10 px-3 py-2 text-[12px] text-warn">
          Offline snapshot · as of {relTime(at)}
        </div>
      )}

      {window && <DeadlineStrip window={window} openCount={openBreaks.length} unresolved={unresolved} />}

      {/* Work tiles — the queue reframed as work, not a report. */}
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-5">
        <StatTile
          label="Open breaks"
          value={openBreaks.length}
          accent="regulatory"
          icon={ShieldAlert}
          sub={
            openBreaks.length ? (
              <span className="text-destructive">{fmtAmount(unresolved)} unresolved</span>
            ) : (
              <StatusBadge variant="ok">all triaged</StatusBadge>
            )
          }
        />
        <StatTile
          label="Reported"
          value={fmtAmount(s.reportedTotal)}
          accent="regulatory"
          sub={`computed ${fmtAmount(s.computedTotal)}`}
        />
        <StatTile
          label="Net difference"
          value={<span className={s.netDiff >= 0 ? "text-debit" : "text-credit"}>{fmtSigned(s.netDiff)}</span>}
          accent="regulatory"
          sub={`${fmtAmount(s.breakAmount)} across breaks`}
        />
        <StatTile
          label="Worksheet"
          value={`${dispositioned}/${breaks.length}`}
          accent="regulatory"
          icon={Check}
          sub="breaks dispositioned"
        />
        <StatTile
          label="Recon"
          value={s.matched}
          accent="regulatory"
          sub={`matched · ${s.missingComputed + s.missingReported} missing`}
        />
      </div>

      <div className="flex items-center gap-3">
        <Segmented
          value={tab}
          onChange={(v) => setTab(v)}
          options={[
            { id: "breaks", label: `Breaks (${breaks.length})` },
            { id: "netting", label: "Netting" },
            { id: "all", label: `All penalties (${work.length})` },
          ]}
        />
        <span className="text-[11px] text-muted-foreground">
          Dispositions are a session worksheet (this browser) — not written back to the CSD.
        </span>
      </div>

      {tab === "breaks" && (
        <BreakWork
          breaks={breaks}
          selected={selected}
          period={period}
          onSelect={(w) => onRoute({ view: "csdr", txnRef: w.line.transactionRef })}
          onOpenParser={() => onRoute({ view: "parser" })}
        />
      )}
      {tab === "netting" && <NettingView cells={cells} />}
      {tab === "all" && <AllLines work={work} />}
    </Shell>
  );
}

// ---------------------------------------------------------------------------
function Shell({
  children,
  onRefresh,
  actions,
}: {
  children: React.ReactNode;
  onRefresh: () => void;
  actions?: React.ReactNode;
}) {
  return (
    <div className="flex h-full flex-col">
      <PlaneHeader
        plane="regulatory"
        planeLabel="Regulatory"
        route="GET /csdr/snapshot"
        title="CSDR penalties"
        summary="Reconcile expected settlement-fail penalties (from MT537 fails) against the CSD's monthly statement, triage the breaks, and disposition them before the dispute cutoff. Starter framework — rates & dates not certified."
        actions={
          <div className="flex items-center gap-2">
            {actions}
            <Btn onClick={onRefresh} className="inline-flex items-center gap-1.5">
              <RefreshCw className="h-3.5 w-3.5" /> Refresh
            </Btn>
          </div>
        }
      />
      <div className="min-h-0 flex-1 space-y-4 overflow-auto p-5">{children}</div>
    </div>
  );
}

// ---------------------------------------------------------------------------
function DeadlineStrip({
  window,
  openCount,
  unresolved,
}: {
  window: DisputeWindow;
  openCount: number;
  unresolved: number;
}) {
  const tone =
    window.state === "closing"
      ? "border-destructive/50 bg-destructive/10 text-destructive"
      : window.state === "closed"
        ? "border-outline-strong bg-surface-toolbar text-muted-foreground"
        : "border-info/45 bg-info/10 text-info";
  const cutoff = window.cutoff.toISOString().slice(0, 10);
  const label =
    window.state === "upcoming"
      ? "Statement pending"
      : window.state === "closed"
        ? "Dispute window closed"
        : `Business day ${window.businessDay} of 10 · ${window.businessDaysRemaining} to dispute cutoff`;
  return (
    <div className={cn("flex flex-wrap items-center gap-x-4 gap-y-1 rounded-lg border px-3.5 py-2.5 text-[12.5px]", tone)}>
      <CalendarClock className="h-4 w-4 shrink-0" />
      <span className="font-semibold">Penalty period {window.period}</span>
      <span>{label}</span>
      <span className="font-mono text-[11.5px] opacity-80">cutoff {cutoff}</span>
      {window.state !== "closed" && openCount > 0 && (
        <span className="ml-auto font-medium">
          {openCount} break{openCount === 1 ? "" : "s"} · {fmtAmount(unresolved)} still to disposition
        </span>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
function BreakWork({
  breaks,
  selected,
  period,
  onSelect,
  onOpenParser,
}: {
  breaks: WorkLine[];
  selected?: WorkLine;
  period: string | null;
  onSelect: (w: WorkLine) => void;
  onOpenParser: () => void;
}) {
  const [reason, setReason] = useState<BreakReason | "all">("all");
  const [dispo, setDispo] = useState<Disposition | "all">("all");
  const [minAmt, setMinAmt] = useState("");

  const filtered = breaks.filter((w) => {
    if (reason !== "all" && w.reason !== reason) return false;
    if (dispo !== "all" && w.disposition !== dispo) return false;
    if (minAmt && w.absDiff < Number(minAmt)) return false;
    return true;
  });

  const reasons = [...new Set(breaks.map((w) => w.reason))];

  const columns: Column<WorkLine>[] = [
    { key: "ref", header: "Txn ref", mono: true, render: (w) => w.line.transactionRef, sortValue: (w) => w.line.transactionRef },
    { key: "isin", header: "ISIN", mono: true, render: (w) => w.line.isin },
    { key: "cp", header: "Counterparty", mono: true, render: (w) => w.line.counterparty ?? "—" },
    {
      key: "reason",
      header: "Reason",
      render: (w) => <span className={cn("text-[11px]", REASON_TONE[w.reason] === "error" && "text-destructive")}>{REASON_LABEL[w.reason]}</span>,
      sortValue: (w) => w.reason,
    },
    { key: "computed", header: "Computed", align: "right", mono: true, render: (w) => fmtAmount(w.line.computed), sortValue: (w) => w.line.computed },
    { key: "reported", header: "Reported", align: "right", mono: true, render: (w) => fmtAmount(w.line.reported), sortValue: (w) => w.line.reported },
    {
      key: "diff",
      header: "Diff",
      align: "right",
      mono: true,
      render: (w) => <span className={w.line.diff >= 0 ? "text-debit" : "text-credit"}>{fmtSigned(w.line.diff)}</span>,
      sortValue: (w) => w.absDiff,
    },
    { key: "dispo", header: "State", render: (w) => <StatusBadge variant={DISPO_VARIANT[w.disposition]}>{w.disposition}</StatusBadge> },
  ];

  return (
    <div className="flex min-h-0 gap-4">
      <div className="min-w-0 flex-1 space-y-2">
        <div className="flex flex-wrap items-center gap-2">
          <Filter label="Reason" value={reason} onChange={(v) => setReason(v as BreakReason | "all")} options={["all", ...reasons]} fmt={(r) => (r === "all" ? "All reasons" : REASON_LABEL[r as BreakReason])} />
          <Filter label="State" value={dispo} onChange={(v) => setDispo(v as Disposition | "all")} options={["all", "open", "investigating", "accepted", "flagged"]} fmt={(d) => (d === "all" ? "All states" : d)} />
          <input
            value={minAmt}
            onChange={(e) => setMinAmt(e.target.value.replace(/[^\d.]/g, ""))}
            placeholder="min |diff|"
            className="w-24 rounded-md border border-outline-strong bg-surface-input px-2 py-1 text-[11.5px] outline-none focus:border-plane-regulatory/50"
          />
          <span className="text-[11px] text-muted-foreground">{filtered.length} of {breaks.length} · worst first</span>
        </div>
        <DataTable
          columns={columns}
          rows={[...filtered].sort((a, b) => b.absDiff - a.absDiff)}
          rowKey={(w) => w.key}
          onRowClick={onSelect}
          activeRow={(w) => selected?.key === w.key}
          empty="No breaks match — the recon is clean for this filter."
          dense
        />
      </div>
      <BreakInspector w={selected} period={period} onOpenParser={onOpenParser} />
    </div>
  );
}

function Filter({
  label,
  value,
  onChange,
  options,
  fmt,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  options: string[];
  fmt: (v: string) => string;
}) {
  return (
    <label className="inline-flex items-center gap-1.5 text-[11px] text-muted-foreground">
      {label}
      <select
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="rounded-md border border-outline-strong bg-surface-input px-1.5 py-1 text-[11.5px] text-foreground outline-none focus:border-plane-regulatory/50"
      >
        {options.map((o) => (
          <option key={o} value={o}>
            {fmt(o)}
          </option>
        ))}
      </select>
    </label>
  );
}

// ---------------------------------------------------------------------------
function BreakInspector({ w, period, onOpenParser }: { w?: WorkLine; period: string | null; onOpenParser: () => void }) {
  if (!w) {
    return (
      <aside className="hidden w-[320px] shrink-0 flex-col items-center justify-center rounded-lg border border-dashed border-outline-subtle p-6 text-center lg:flex">
        <Gavel className="h-6 w-6 text-plane-regulatory/60" />
        <p className="mt-2 text-[12.5px] font-medium">Select a break</p>
        <p className="mt-1 text-[11.5px] text-muted-foreground">Pick a row to diagnose it and record a disposition.</p>
      </aside>
    );
  }
  const a = w.accrual;
  const impliedBps = a && a.referenceAmount > 0 ? (w.line.reported / a.referenceAmount) * 10000 : null;
  return (
    <aside className="w-[320px] shrink-0 space-y-3 overflow-auto rounded-lg border border-outline-subtle bg-surface-panel p-3">
      <div>
        <div className="flex items-center gap-2">
          <span className="font-mono text-[13px] font-semibold">{w.line.transactionRef}</span>
          <StatusBadge variant={STATUS_VARIANT[w.line.status] ?? "neutral"}>{w.line.status.replace("_", " ")}</StatusBadge>
        </div>
        <div className="mt-1 flex items-center gap-1.5">
          <TagChip tag={w.line.penaltyType} tone={REASON_TONE[w.reason]} />
          <span className="text-[11.5px] text-muted-foreground">{REASON_LABEL[w.reason]}</span>
        </div>
        <p className="mt-1.5 text-[11.5px] leading-snug text-foreground/80">{w.reasonDetail}</p>
      </div>

      <Panel title="Computed vs reported">
        <Row label="Computed" value={`${w.line.currency} ${fmtAmount(w.line.computed)}`} />
        <Row label="Reported" value={`${w.line.currency} ${fmtAmount(w.line.reported)}`} />
        <Row label="Diff" value={fmtSigned(w.line.diff)} tone={w.line.diff >= 0 ? "debit" : "credit"} />
      </Panel>

      {a && (
        <Panel title="Accrual inputs">
          <Row label="ISIN" value={a.isin} mono />
          <Row label="Instrument" value={a.instrumentType.replace(/_/g, " ")} />
          <Row label="Our rate" value={`${a.penaltyRateBps} bps`} />
          {impliedBps != null && <Row label="Implied rate" value={`${impliedBps.toFixed(2)} bps`} tone={Math.abs(impliedBps - a.penaltyRateBps) / a.penaltyRateBps > 0.05 ? "debit" : undefined} />}
          <Row label="Reference amt" value={fmtAmount(a.referenceAmount)} mono />
          <Row label="ISD" value={a.intendedSettlementDate ?? "—"} mono />
          <Row label="Direction" value={a.direction} />
        </Panel>
      )}

      <button
        type="button"
        onClick={onOpenParser}
        className="inline-flex items-center gap-1 text-[11.5px] font-medium text-plane-regulatory hover:underline"
      >
        Open MT537 in Parser <ArrowRight className="h-3 w-3" />
      </button>

      <div>
        <div className="mb-1.5 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Disposition</div>
        <div className="grid grid-cols-2 gap-1.5">
          <DispoBtn active={w.disposition === "investigating"} icon={Eye} label="Investigate" onClick={() => period && setDisposition(period, w.key, "investigating", w.note)} />
          <DispoBtn active={w.disposition === "accepted"} icon={Check} label="Accept" onClick={() => period && setDisposition(period, w.key, "accepted", w.note)} />
          <DispoBtn active={w.disposition === "flagged"} icon={Flag} label="Flag dispute" onClick={() => period && setDisposition(period, w.key, "flagged", w.note)} />
          <DispoBtn active={w.disposition === "open"} icon={ShieldAlert} label="Reopen" onClick={() => period && setDisposition(period, w.key, "open", w.note)} />
        </div>
        <textarea
          value={w.note ?? ""}
          onChange={(e) => period && setDisposition(period, w.key, w.disposition, e.target.value)}
          placeholder="Investigation note (worksheet)…"
          className="mt-2 h-16 w-full resize-none rounded-md border border-outline-strong bg-surface-input p-2 text-[11.5px] outline-none focus:border-plane-regulatory/50"
        />
      </div>
    </aside>
  );
}

function DispoBtn({ active, icon: Icon, label, onClick }: { active: boolean; icon: typeof Eye; label: string; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "inline-flex items-center gap-1.5 rounded-md border px-2 py-1.5 text-[11px] transition-colors",
        active ? "border-plane-regulatory/60 bg-plane-regulatory/12 text-plane-regulatory" : "border-outline-strong text-foreground/80 hover:bg-surface-hover",
      )}
    >
      <Icon className="h-3.5 w-3.5" /> {label}
    </button>
  );
}

function Panel({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="rounded-lg border border-outline-subtle bg-surface-app p-2.5">
      <div className="mb-1.5 text-[10.5px] font-semibold uppercase tracking-wider text-muted-foreground">{title}</div>
      <div className="space-y-1">{children}</div>
    </section>
  );
}

function Row({ label, value, mono, tone }: { label: string; value: string; mono?: boolean; tone?: "credit" | "debit" }) {
  return (
    <div className="flex items-center justify-between text-[11.5px]">
      <span className="text-muted-foreground">{label}</span>
      <span className={cn(mono && "font-mono tabular-nums", tone === "credit" && "text-credit", tone === "debit" && "text-debit")}>{value}</span>
    </div>
  );
}

// ---------------------------------------------------------------------------
function NettingView({ cells }: { cells: ReturnType<typeof netting> }) {
  const totals = cells.reduce(
    (a, c) => ({ computed: a.computed + c.computed, reported: a.reported + c.reported, adj: a.adj + c.disputeAdjusted }),
    { computed: 0, reported: 0, adj: 0 },
  );
  const columns: Column<(typeof cells)[number]>[] = [
    { key: "cp", header: "Counterparty", mono: true, render: (c) => c.counterparty },
    { key: "ccy", header: "Ccy", render: (c) => c.currency },
    { key: "computed", header: "Computed", align: "right", mono: true, render: (c) => fmtAmount(c.computed), sortValue: (c) => c.computed },
    { key: "reported", header: "Reported (CSD)", align: "right", mono: true, render: (c) => fmtAmount(c.reported), sortValue: (c) => c.reported },
    {
      key: "adj",
      header: "If disputes upheld",
      align: "right",
      mono: true,
      render: (c) => (
        <span className={c.disputeAdjusted !== c.reported ? "text-plane-regulatory" : ""}>{fmtAmount(c.disputeAdjusted)}</span>
      ),
      sortValue: (c) => c.disputeAdjusted,
    },
    { key: "open", header: "Open breaks", align: "right", render: (c) => (c.openBreaks ? <StatusBadge variant="warn">{c.openBreaks}</StatusBadge> : "—") },
  ];
  return (
    <div className="space-y-2">
      <p className="text-[12px] text-muted-foreground">
        Net penalty payable per counterparty per currency — reported by the CSD, and adjusted for the position if every
        flagged dispute is upheld.
      </p>
      <DataTable columns={columns} rows={cells} rowKey={(c) => `${c.counterparty}|${c.currency}`} empty="No penalties this period." dense />
      <div className="flex justify-end gap-8 rounded-lg border border-outline-subtle bg-surface-panel px-4 py-2 text-[12px]">
        <span className="text-muted-foreground">Totals</span>
        <span className="font-mono tabular-nums">computed {fmtAmount(totals.computed)}</span>
        <span className="font-mono tabular-nums">reported {fmtAmount(totals.reported)}</span>
        <span className="font-mono tabular-nums text-plane-regulatory">if upheld {fmtAmount(totals.adj)}</span>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
function AllLines({ work }: { work: WorkLine[] }) {
  const columns: Column<WorkLine>[] = [
    { key: "ref", header: "Txn ref", mono: true, render: (w) => w.line.transactionRef, sortValue: (w) => w.line.transactionRef },
    { key: "isin", header: "ISIN", mono: true, render: (w) => w.line.isin },
    { key: "cp", header: "Counterparty", mono: true, render: (w) => w.line.counterparty ?? "—" },
    { key: "type", header: "Type", render: (w) => w.line.penaltyType },
    { key: "computed", header: "Computed", align: "right", mono: true, render: (w) => fmtAmount(w.line.computed), sortValue: (w) => w.line.computed },
    { key: "reported", header: "Reported", align: "right", mono: true, render: (w) => fmtAmount(w.line.reported), sortValue: (w) => w.line.reported },
    { key: "diff", header: "Diff", align: "right", mono: true, render: (w) => <span className={w.line.diff >= 0 ? "text-debit" : "text-credit"}>{fmtSigned(w.line.diff)}</span>, sortValue: (w) => w.absDiff },
    { key: "status", header: "Status", render: (w) => <StatusBadge variant={STATUS_VARIANT[w.line.status] ?? "neutral"}>{w.line.status.replace("_", " ")}</StatusBadge> },
  ];
  return <DataTable columns={columns} rows={work} rowKey={(w) => w.key} empty="No penalties this period." dense />;
}
