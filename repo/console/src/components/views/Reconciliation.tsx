import { useMemo } from "react";
import { cn } from "@/lib/utils";
import { PlaneHeader } from "@/components/PlaneHeader";
import { StatTile } from "@/components/StatTile";
import { DataTable, type Column } from "@/components/DataTable";
import { StatusBadge } from "@/components/StatusBadge";
import { TagChip } from "@/components/TagChip";
import { NodeErrorBanner } from "@/components/NodeErrorBanner";
import { PillarEmpty } from "@/components/PillarEmpty";
import { Btn } from "@/components/setup/kit";
import { useSnapshot } from "@/lib/useSnapshot";
import { verdict, cashPnl } from "@/lib/recon";
import { fmtAmount, fmtSigned, relTime } from "@/lib/format";
import type { AppRoute } from "@/lib/route";
import type { SnapshotStatement, SnapshotEntry } from "@/lib/types";
import { RefreshCw, Landmark, ScrollText } from "lucide-react";

export function Reconciliation({
  route,
  onRoute,
}: {
  route: Extract<AppRoute, { view: "recon" }>;
  onRoute: (r: AppRoute) => void;
}) {
  const { result, error, at, refresh } = useSnapshot();
  const snapshot = result?.snapshot;
  const pnl = useMemo(() => (snapshot ? cashPnl(snapshot) : null), [snapshot]);

  const selected = useMemo(
    () => (route.messageId ? snapshot?.statements.find((s) => s.messageId === route.messageId) : undefined),
    [snapshot, route.messageId],
  );

  const selectStatement = (s: SnapshotStatement) => {
    onRoute({ view: "recon", messageId: s.messageId === route.messageId ? undefined : s.messageId });
  };

  const columns: Column<SnapshotStatement>[] = [
    { key: "source", header: "Source", render: (s) => s.source, sortValue: (s) => s.source },
    {
      key: "messageId",
      header: "Message",
      mono: true,
      render: (s) => s.messageId,
      sortValue: (s) => s.messageId,
    },
    {
      key: "account",
      header: "Account",
      mono: true,
      render: (s) => s.account,
      sortValue: (s) => s.account,
    },
    { key: "currency", header: "Ccy", render: (s) => s.currency, sortValue: (s) => s.currency },
    {
      key: "opening",
      header: "Opening",
      align: "right",
      mono: true,
      render: (s) => fmtAmount(s.opening),
      sortValue: (s) => s.opening,
    },
    {
      key: "net",
      header: "Net",
      align: "right",
      mono: true,
      render: (s) => {
        const v = verdict(s);
        return <span className={v.net >= 0 ? "text-credit" : "text-debit"}>{fmtSigned(v.net)}</span>;
      },
      sortValue: (s) => verdict(s).net,
    },
    {
      key: "closing",
      header: "Closing",
      align: "right",
      mono: true,
      render: (s) => fmtAmount(s.closing),
      sortValue: (s) => s.closing,
    },
    {
      key: "reconciles",
      header: "Status",
      align: "right",
      render: (s) =>
        verdict(s).reconciles ? (
          <StatusBadge variant="ok">reconciles</StatusBadge>
        ) : (
          <StatusBadge variant="error">break</StatusBadge>
        ),
      sortValue: (s) => (verdict(s).reconciles ? 1 : 0),
    },
  ];

  const entryColumns: Column<SnapshotEntry>[] = [
    { key: "valueDate", header: "Value date", render: (e) => e.valueDate, sortValue: (e) => e.valueDate },
    { key: "direction", header: "Direction", render: (e) => <DirectionTag direction={e.direction} /> },
    {
      key: "transactionType",
      header: "Type",
      render: (e) => <TagChip tag={e.transactionType || "—"} />,
      sortValue: (e) => e.transactionType,
    },
    {
      key: "amount",
      header: "Amount",
      align: "right",
      mono: true,
      render: (e) => fmtAmount(e.amount),
      sortValue: (e) => e.amount,
    },
    {
      key: "signedAmount",
      header: "Signed",
      align: "right",
      mono: true,
      render: (e) => (
        <span className={e.signedAmount >= 0 ? "text-credit" : "text-debit"}>{fmtSigned(e.signedAmount)}</span>
      ),
      sortValue: (e) => e.signedAmount,
    },
    {
      key: "reference",
      header: "Reference",
      mono: true,
      render: (e) => e.reference || "—",
      sortValue: (e) => e.reference ?? "",
    },
    { key: "info", header: "Info", render: (e) => e.info || "—" },
  ];

  return (
    <div className="flex h-full flex-col">
      <PlaneHeader
        plane="books"
        planeLabel="Books"
        route="GET /recon/snapshot"
        title="Reconciliation"
        summary="Cash statements, Tier-1 P&L, and reconciliation breaks."
        actions={
          <Btn onClick={refresh} className="inline-flex items-center gap-1.5">
            <RefreshCw className="h-3.5 w-3.5" /> Refresh
          </Btn>
        }
      />

      <div className="min-h-0 flex-1 overflow-auto p-5 space-y-4">
        {result?.stale && (
          <div className="flex items-center gap-2 rounded-lg border border-warn/45 bg-warn/10 px-3 py-2 text-[12px] text-warn">
            Offline snapshot · as of {relTime(at)}
          </div>
        )}

        {error && !result && <NodeErrorBanner error={error} />}

        {!snapshot && !error && (
          <div className="rounded-lg border border-dashed border-outline-subtle p-8 text-center text-[12.5px] text-muted-foreground">
            Loading recon snapshot…
          </div>
        )}

        {!snapshot && error && (
          <PillarEmpty
            icon={Landmark}
            title="Recon read-model unreachable"
            body="Recon read-model unreachable — start `ingest serve`."
          />
        )}

        {snapshot && pnl && (
          <>
            <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-6">
              <StatTile
                label="Inflows"
                value={<span className="text-credit">{fmtAmount(pnl.inflows)}</span>}
                accent="books"
              />
              <StatTile
                label="Outflows"
                value={<span className="text-debit">{fmtAmount(pnl.outflows)}</span>}
                accent="books"
              />
              <StatTile
                label="Net movement"
                value={<span className={pnl.net >= 0 ? "text-credit" : "text-debit"}>{fmtSigned(pnl.net)}</span>}
                accent="books"
              />
              <StatTile
                label="Breaks"
                value={pnl.breaks}
                accent="books"
                sub={
                  pnl.breaks > 0 ? (
                    <StatusBadge variant="error">{pnl.breaks} unreconciled</StatusBadge>
                  ) : (
                    <StatusBadge variant="ok">all reconciled</StatusBadge>
                  )
                }
              />
              <StatTile label="Statements" value={pnl.statements} accent="books" />
              <StatTile label="Entries" value={pnl.entries} accent="books" />
            </div>

            {pnl.byType.length > 0 && (
              <section className="rounded-lg border border-outline-subtle bg-surface-panel p-3.5">
                <div className="mb-2.5 flex items-center gap-1.5">
                  <ScrollText className="h-3.5 w-3.5 text-plane-books" />
                  <span className="text-[10.5px] font-semibold uppercase tracking-wider text-muted-foreground">
                    By transaction type
                  </span>
                </div>
                <div className="space-y-1.5">
                  {(() => {
                    const max = Math.max(...pnl.byType.map((t) => Math.abs(t.amount)), 1);
                    return pnl.byType.map((t) => (
                      <div key={t.type} className="flex items-center gap-2.5">
                        <TagChip tag={t.type} className="shrink-0" />
                        <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-surface-toolbar">
                          <div
                            className={cn("h-full rounded-full", t.amount >= 0 ? "bg-credit/60" : "bg-debit/60")}
                            style={{ width: `${(Math.abs(t.amount) / max) * 100}%` }}
                          />
                        </div>
                        <span
                          className={cn(
                            "w-28 shrink-0 text-right font-mono text-[11.5px] tabular-nums",
                            t.amount >= 0 ? "text-credit" : "text-debit",
                          )}
                        >
                          {fmtSigned(t.amount)}
                        </span>
                      </div>
                    ));
                  })()}
                </div>
              </section>
            )}

            <DataTable
              columns={columns}
              rows={snapshot.statements}
              rowKey={(s) => s.messageId}
              onRowClick={selectStatement}
              activeRow={(s) => s.messageId === route.messageId}
              empty="No cash statements in the snapshot."
            />

            {selected && <StatementDrilldown statement={selected} entryColumns={entryColumns} />}
          </>
        )}
      </div>
    </div>
  );
}

function DirectionTag({ direction }: { direction: string }) {
  const credit = direction.includes("credit");
  return (
    <span
      className={cn(
        "inline-flex items-center rounded border px-1.5 py-0.5 text-[10.5px] font-medium capitalize",
        credit ? "border-credit/40 bg-credit/10 text-credit" : "border-debit/40 bg-debit/10 text-debit",
      )}
    >
      {direction.replace(/_/g, " ")}
    </span>
  );
}

function StatementDrilldown({
  statement,
  entryColumns,
}: {
  statement: SnapshotStatement;
  entryColumns: Column<SnapshotEntry>[];
}) {
  const v = verdict(statement);
  return (
    <section className="space-y-2.5 rounded-lg border border-outline-subtle bg-surface-panel p-3.5">
      <div className="flex items-center justify-between gap-2">
        <div className="min-w-0">
          <div className="font-mono text-[12.5px] font-semibold text-foreground">{statement.messageId}</div>
          <div className="text-[11px] text-muted-foreground">
            {statement.source} · {statement.account} · {statement.currency}
          </div>
        </div>
      </div>

      <div
        className={cn(
          "flex flex-wrap items-center gap-x-2 gap-y-1 rounded-lg border px-3.5 py-2.5 font-mono text-[12.5px]",
          v.reconciles ? "border-ok/40 bg-ok/8" : "border-destructive/45 bg-destructive/8",
        )}
      >
        <span className="text-muted-foreground">opening</span>
        <span className="tabular-nums text-foreground">{fmtAmount(statement.opening)}</span>
        <span className="text-muted-foreground">+ Σ {statement.entries.length} entries</span>
        <span className={cn("tabular-nums", v.net >= 0 ? "text-credit" : "text-debit")}>({fmtSigned(v.net)})</span>
        <span className="text-muted-foreground">=</span>
        <span className="tabular-nums text-foreground">{fmtAmount(statement.closing)}</span>
        <span className="ml-auto inline-flex items-center gap-1.5">
          {v.reconciles ? (
            <StatusBadge variant="ok">✓ reconciles</StatusBadge>
          ) : (
            <StatusBadge variant="error">✗ break {fmtSigned(v.diff)}</StatusBadge>
          )}
        </span>
      </div>

      <DataTable
        columns={entryColumns}
        rows={statement.entries}
        rowKey={(_e, i) => `${statement.messageId}-${i}`}
        empty="No entries on this statement."
        dense
      />
    </section>
  );
}
