// Landing dashboard — pipeline health (swift-api) + books health (recon
// read-model) at a glance, plus a way in to the two most common next steps
// (parse a message, drill into a job or a break).
import { useEffect, useMemo, useState } from "react";
import { PlaneHeader } from "@/components/PlaneHeader";
import { StatTile } from "@/components/StatTile";
import { DataTable, type Column } from "@/components/DataTable";
import { StatusBadge, type StateVariant } from "@/components/StatusBadge";
import { NodeErrorBanner } from "@/components/NodeErrorBanner";
import { PillarEmpty } from "@/components/PillarEmpty";
import { Btn, errText } from "@/components/setup/kit";
import { getMetrics, listJobs } from "@/lib/api";
import { useSnapshot } from "@/lib/useSnapshot";
import { cashPnl } from "@/lib/recon";
import { fmtInt, fmtSigned, relTime } from "@/lib/format";
import type { AppRoute } from "@/lib/route";
import type { JobStatus, JobStatusView, Metrics } from "@/lib/types";
import {
  Activity,
  AlertTriangle,
  ArrowLeftRight,
  CheckCircle2,
  Database,
  Layers,
  ScanLine,
  Wallet,
  XCircle,
} from "lucide-react";

const JOB_STATUS_BADGE: Record<JobStatus, { variant: StateVariant; pulse?: boolean }> = {
  queued: { variant: "neutral" },
  running: { variant: "info", pulse: true },
  completed: { variant: "ok" },
  completed_with_errors: { variant: "warn" },
  failed: { variant: "error" },
};

export function Overview({ onRoute }: { onRoute: (r: AppRoute) => void }) {
  const [metrics, setMetrics] = useState<Metrics | null>(null);
  const [metricsError, setMetricsError] = useState<string | null>(null);
  const [jobs, setJobs] = useState<JobStatusView[]>([]);

  useEffect(() => {
    let live = true;
    const tick = () =>
      getMetrics()
        .then((m) => {
          if (!live) return;
          setMetrics(m);
          setMetricsError(null);
        })
        .catch((e) => live && setMetricsError(errText(e)));
    tick();
    const t = setInterval(tick, 4000);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, []);

  useEffect(() => {
    let live = true;
    const tick = () =>
      listJobs(6)
        .then((j) => live && setJobs(j))
        .catch(() => {
          /* jobs table just stays at last-known-good; metrics banner covers the outage */
        });
    tick();
    const t = setInterval(tick, 4000);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, []);

  const { result } = useSnapshot();
  const snapshot = result?.snapshot;
  const pnl = useMemo(() => (snapshot ? cashPnl(snapshot) : null), [snapshot]);

  const sourceMix = useMemo(() => {
    if (!snapshot) return [];
    const counts = new Map<string, number>();
    for (const s of snapshot.statements) counts.set(s.source, (counts.get(s.source) ?? 0) + 1);
    for (const p of snapshot.positions) counts.set(p.source, (counts.get(p.source) ?? 0) + 1);
    const max = Math.max(1, ...counts.values());
    return [...counts.entries()]
      .map(([source, count]) => ({ source, count, pct: Math.round((count / max) * 100) }))
      .sort((a, b) => b.count - a.count);
  }, [snapshot]);

  const jobColumns: Column<JobStatusView>[] = [
    { key: "job_id", header: "Job", mono: true, render: (r) => r.job_id.slice(0, 12) },
    {
      key: "status",
      header: "Status",
      render: (r) => {
        const { variant, pulse } = JOB_STATUS_BADGE[r.status];
        return (
          <StatusBadge variant={variant} pulse={pulse}>
            {r.status.replace(/_/g, " ")}
          </StatusBadge>
        );
      },
    },
    {
      key: "updated_at",
      header: "Updated",
      align: "right",
      render: (r) => relTime(r.updated_at),
      sortValue: (r) => Date.parse(r.updated_at),
    },
  ];

  return (
    <div className="flex h-full flex-col">
      <PlaneHeader
        plane="pipeline"
        planeLabel="Pipeline"
        route="swift-api + recon"
        title="Overview"
        summary="Ingestion health, throughput, and reconciliation at a glance."
        actions={
          <Btn variant="primary" onClick={() => onRoute({ view: "parser" })} className="inline-flex items-center gap-1.5">
            <ScanLine className="h-3.5 w-3.5" /> Parse a message
          </Btn>
        }
      />

      <div className="min-h-0 flex-1 overflow-auto p-5 space-y-4">
        <NodeErrorBanner error={metricsError} />

        {/* Pipeline throughput */}
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
          <StatTile
            label="Jobs in-flight"
            value={metrics ? fmtInt(metrics.jobs_in_flight) : "—"}
            icon={Activity}
            accent="pipeline"
          />
          <StatTile
            label="Completed"
            value={metrics ? fmtInt(metrics.jobs_completed) : "—"}
            icon={CheckCircle2}
            accent="pipeline"
          />
          <StatTile
            label="Failed"
            value={metrics ? fmtInt(metrics.jobs_failed) : "—"}
            icon={XCircle}
            accent="system"
          />
          <StatTile
            label="Queue depth"
            value={metrics ? fmtInt(metrics.queue_depth) : "—"}
            icon={Layers}
            accent="pipeline"
          />
        </div>

        {/* Books health */}
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
          <StatTile
            label="Statements"
            value={snapshot ? fmtInt(snapshot.statements.length) : "—"}
            icon={Database}
            accent="books"
            onClick={() => onRoute({ view: "recon" })}
          />
          <StatTile
            label="Positions"
            value={snapshot ? fmtInt(snapshot.positions.length) : "—"}
            icon={Wallet}
            accent="books"
            onClick={() => onRoute({ view: "positions" })}
          />
          <StatTile
            label="Reconciliation breaks"
            value={pnl ? fmtInt(pnl.breaks) : "—"}
            sub={pnl && pnl.breaks > 0 ? <StatusBadge variant="warn">{pnl.breaks} unreconciled</StatusBadge> : undefined}
            icon={AlertTriangle}
            accent="books"
            onClick={() => onRoute({ view: "recon" })}
          />
          <StatTile
            label="Net cash movement"
            value={
              pnl ? <span className={pnl.net >= 0 ? "text-credit" : "text-debit"}>{fmtSigned(pnl.net)}</span> : "—"
            }
            icon={ArrowLeftRight}
            accent="books"
          />
        </div>

        {/* Recent jobs */}
        <section>
          <h2 className="mb-2 text-[10.5px] font-semibold uppercase tracking-wider text-muted-foreground">
            Recent jobs
          </h2>
          <DataTable
            columns={jobColumns}
            rows={jobs}
            rowKey={(r) => r.job_id}
            onRowClick={(r) => onRoute({ view: "jobs", jobId: r.job_id })}
            empty="No jobs yet — submit a message to get started."
          />
        </section>

        {/* Source mix */}
        <section>
          <h2 className="mb-2 text-[10.5px] font-semibold uppercase tracking-wider text-muted-foreground">
            Source mix
          </h2>
          {sourceMix.length ? (
            <div className="space-y-2 rounded-lg border border-outline-subtle bg-surface-panel p-3">
              {sourceMix.map((s) => (
                <div key={s.source} className="flex items-center gap-3">
                  <span className="w-28 shrink-0 truncate font-mono text-[11.5px] text-foreground">{s.source}</span>
                  <div className="h-2 flex-1 overflow-hidden rounded-full bg-surface-toolbar">
                    <div className="h-full rounded-full bg-plane-control" style={{ width: `${s.pct}%` }} />
                  </div>
                  <span className="w-10 shrink-0 text-right font-mono text-[11.5px] tabular-nums text-muted-foreground">
                    {fmtInt(s.count)}
                  </span>
                </div>
              ))}
            </div>
          ) : (
            <PillarEmpty
              icon={Database}
              title="No source data yet"
              body="Ingest a statement or position message to see the source mix build up here."
            />
          )}
        </section>
      </div>
    </div>
  );
}
