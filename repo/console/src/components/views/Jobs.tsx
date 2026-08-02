// Pipeline plane: master–detail operator view over swift-api jobs. Left rail
// lists submitted jobs (polled); the right panel is a deep-linkable manifest
// inspector — counts, timings, artifacts (fetched lazily), and the per-message
// table that hands off to the Parser view for a single-message inspect.
import { useEffect, useState } from "react";
import { PlaneHeader } from "@/components/PlaneHeader";
import { DataTable, type Column } from "@/components/DataTable";
import { StatTile } from "@/components/StatTile";
import { StatusBadge, type StateVariant } from "@/components/StatusBadge";
import { TagChip } from "@/components/TagChip";
import { NodeErrorBanner } from "@/components/NodeErrorBanner";
import { Card, Btn, Field, inputCls, errText } from "@/components/setup/kit";
import { listJobs, submitJob, getManifest, getObject } from "@/lib/api";
import { fmtMs, relTime } from "@/lib/format";
import { toast } from "@/lib/toast";
import { isTerminal, type JobManifest, type JobStatus, type JobStatusView, type MessageEntry } from "@/lib/types";
import type { AppRoute } from "@/lib/route";
import { Archive, FileText, FileWarning, Inbox, Plus } from "lucide-react";

const STATUS_VARIANT: Record<JobStatus, { variant: StateVariant; pulse?: boolean }> = {
  completed: { variant: "ok" },
  completed_with_errors: { variant: "warn" },
  failed: { variant: "error" },
  running: { variant: "info", pulse: true },
  queued: { variant: "neutral" },
};

export function Jobs({ route, onRoute }: { route: Extract<AppRoute, { view: "jobs" }>; onRoute: (r: AppRoute) => void }) {
  const [jobs, setJobs] = useState<JobStatusView[]>([]);
  const [listError, setListError] = useState<string | null>(null);
  const [showSubmit, setShowSubmit] = useState(false);

  useEffect(() => {
    let live = true;
    const tick = () =>
      listJobs(50)
        .then((j) => {
          if (!live) return;
          setJobs(j);
          setListError(null);
        })
        .catch((e) => live && setListError(errText(e)));
    tick();
    const t = setInterval(tick, 4000);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, []);

  const refreshList = () => listJobs(50).then(setJobs).catch(() => {});

  const columns: Column<JobStatusView>[] = [
    { key: "job_id", header: "Job ID", mono: true, render: (r) => r.job_id, sortValue: (r) => r.job_id },
    {
      key: "status",
      header: "Status",
      render: (r) => {
        const s = STATUS_VARIANT[r.status];
        return (
          <StatusBadge variant={s.variant} pulse={s.pulse}>
            {r.status}
          </StatusBadge>
        );
      },
      sortValue: (r) => r.status,
    },
    { key: "created_at", header: "Created", render: (r) => relTime(r.created_at), sortValue: (r) => r.created_at },
  ];

  return (
    <div className="flex h-full flex-col">
      <PlaneHeader
        plane="pipeline"
        planeLabel="Pipeline"
        route="GET /v1/jobs"
        title="Ingest jobs"
        summary="Submitted FIN ingestion jobs, their manifests, and produced artifacts."
        actions={
          <Btn variant="primary" onClick={() => setShowSubmit((s) => !s)} className="inline-flex items-center gap-1.5">
            <Plus className="h-3.5 w-3.5" /> Submit job
          </Btn>
        }
      />
      <div className="min-h-0 flex-1 space-y-4 overflow-auto p-5">
        <NodeErrorBanner error={listError} />

        {showSubmit && (
          <SubmitForm
            onDone={(jobId) => {
              setShowSubmit(false);
              refreshList();
              onRoute({ view: "jobs", jobId });
            }}
          />
        )}

        <div className="flex gap-4">
          <div className="min-w-0 flex-1">
            <DataTable
              columns={columns}
              rows={jobs}
              rowKey={(r) => r.job_id}
              onRowClick={(r) => onRoute({ view: "jobs", jobId: r.job_id })}
              activeRow={(r) => route.jobId === r.job_id}
              empty="No jobs submitted yet."
            />
          </div>

          {route.jobId && (
            <div className="w-[440px] shrink-0">
              <DetailPanel jobId={route.jobId} onRoute={onRoute} />
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
function SubmitForm({ onDone }: { onDone: (jobId: string) => void }) {
  const [prefix, setPrefix] = useState("");
  const [messageType, setMessageType] = useState("");
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    const trimmed = prefix.trim();
    if (!trimmed) {
      toast("warn", "input_prefix is required");
      return;
    }
    setBusy(true);
    try {
      const payload: { input_prefix: string; message_type?: string } = { input_prefix: trimmed };
      if (messageType.trim()) payload.message_type = messageType.trim();
      const sub = await submitJob(payload);
      toast("ok", "Job submitted", sub.job_id);
      onDone(sub.job_id);
    } catch (e) {
      toast("error", "Submit failed", errText(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card title="Submit ingest job" desc="POST /v1/jobs">
      <div className="flex flex-wrap items-end gap-3">
        <div className="min-w-[240px] flex-1">
          <Field label="input_prefix">
            <input
              value={prefix}
              onChange={(e) => setPrefix(e.target.value)}
              placeholder="s3://bucket/prefix/ or local path prefix"
              className={inputCls}
            />
          </Field>
        </div>
        <div className="min-w-[160px]">
          <Field label="message_type (optional)">
            <input
              value={messageType}
              onChange={(e) => setMessageType(e.target.value)}
              placeholder="MT940"
              className={inputCls}
            />
          </Field>
        </div>
        <Btn variant="primary" onClick={submit} disabled={busy || !prefix.trim()}>
          {busy ? "Submitting…" : "Submit"}
        </Btn>
      </div>
    </Card>
  );
}

// ---------------------------------------------------------------------------
function DetailPanel({ jobId, onRoute }: { jobId: string; onRoute: (r: AppRoute) => void }) {
  const [manifest, setManifest] = useState<JobManifest | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [viewing, setViewing] = useState<{ label: string; text: string } | null>(null);
  const [viewError, setViewError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    setManifest(null);
    setError(null);
    setViewing(null);
    setViewError(null);

    const tick = () => {
      getManifest(jobId)
        .then((m) => {
          if (!live) return;
          setManifest(m);
          setError(null);
          if (!isTerminal(m.status)) timer = setTimeout(tick, 4000);
        })
        .catch((e) => {
          if (!live) return;
          setError(errText(e));
        });
    };
    tick();
    return () => {
      live = false;
      if (timer) clearTimeout(timer);
    };
  }, [jobId]);

  const openArtifact = async (label: string, uri: string) => {
    setViewError(null);
    try {
      const text = await getObject(uri);
      setViewing({ label, text });
    } catch (e) {
      setViewError(errText(e));
    }
  };

  if (error) {
    return (
      <Card title={`Job ${jobId.slice(0, 12)}…`}>
        <NodeErrorBanner error={error} />
      </Card>
    );
  }

  if (!manifest) {
    return (
      <Card title={`Job ${jobId.slice(0, 12)}…`}>
        <p className="text-[12px] text-muted-foreground">Loading manifest…</p>
      </Card>
    );
  }

  const s = STATUS_VARIANT[manifest.status];
  const messages = manifest.messages ?? [];
  const msgColumns: Column<MessageEntry & { _idx: number }>[] = [
    { key: "message_id", header: "Message", mono: true, render: (r) => r.message_id ?? `#${r._idx}` },
    {
      key: "message_type",
      header: "Type",
      render: (r) =>
        r.message_type ? <TagChip tag={r.message_type.replace(/^MT/, "")} tone="active" /> : <span className="text-muted-foreground">—</span>,
    },
    {
      key: "status",
      header: "Status",
      render: (r) =>
        r.error ? (
          <StatusBadge variant="error">err</StatusBadge>
        ) : (
          <StatusBadge variant="ok">ok</StatusBadge>
        ),
    },
    {
      key: "action",
      header: "",
      align: "right",
      render: (r) => (
        <Btn onClick={() => onRoute({ view: "parser", jobId, msgIndex: r._idx })}>Inspect</Btn>
      ),
    },
  ];

  return (
    <div className="space-y-3">
      <Card
        title={`Job ${jobId.slice(0, 12)}…`}
        right={
          <StatusBadge variant={s.variant} pulse={s.pulse}>
            {manifest.status}
          </StatusBadge>
        }
      >
        <div className="grid grid-cols-3 gap-2">
          <StatTile label="Messages" value={manifest.counts.messages} accent="pipeline" icon={Inbox} />
          <StatTile label="Rendered" value={manifest.counts.rendered} accent="pipeline" icon={FileText} />
          <StatTile label="Errors" value={manifest.counts.errors} accent="pipeline" icon={FileWarning} />
        </div>

        {manifest.counts.errors > 0 && (
          <div className="mt-3 flex items-center gap-2 rounded-lg border border-warn/45 bg-warn/10 px-2.5 py-1.5 text-[11.5px] text-warn">
            <FileWarning className="h-3.5 w-3.5 shrink-0" />
            {manifest.counts.errors} of {manifest.counts.messages} message(s) failed — see errors artifact or the table below.
          </div>
        )}

        <div className="mt-3 grid grid-cols-2 gap-x-4 gap-y-1 text-[11.5px] sm:grid-cols-3">
          <TimeRow label="parse" ms={manifest.timings.parse_ms} />
          <TimeRow label="render" ms={manifest.timings.render_ms} />
          <TimeRow label="export" ms={manifest.timings.export_ms} />
          <TimeRow label="zip" ms={manifest.timings.zip_ms} />
          <TimeRow label="total" ms={manifest.timings.total_ms} />
        </div>
      </Card>

      <Card title="Artifacts">
        <div className="flex flex-wrap gap-2">
          {manifest.outputs.manifest && (
            <Btn onClick={() => openArtifact("Manifest", manifest.outputs.manifest!)} className="inline-flex items-center gap-1.5">
              <FileText className="h-3.5 w-3.5" /> View manifest
            </Btn>
          )}
          {manifest.outputs.errors_ndjson && (
            <Btn onClick={() => openArtifact("Errors", manifest.outputs.errors_ndjson!)} className="inline-flex items-center gap-1.5">
              <FileWarning className="h-3.5 w-3.5" /> View errors
            </Btn>
          )}
          {manifest.outputs.zip && (
            <Btn onClick={() => setViewing({ label: "Zip", text: manifest.outputs.zip! })} className="inline-flex items-center gap-1.5">
              <Archive className="h-3.5 w-3.5" /> View zip
            </Btn>
          )}
          {!manifest.outputs.manifest && !manifest.outputs.errors_ndjson && !manifest.outputs.zip && (
            <span className="text-[11.5px] text-muted-foreground">No artifacts produced yet.</span>
          )}
        </div>
        {viewError && <NodeErrorBanner error={viewError} className="mt-2" />}
        {viewing && (
          <div className="mt-2">
            <div className="mb-1 text-[10.5px] font-semibold uppercase tracking-wider text-muted-foreground">{viewing.label}</div>
            <pre className="max-h-64 overflow-auto rounded-lg border border-outline-subtle bg-surface-input p-2.5 font-mono text-[11px] leading-relaxed text-foreground/85">
              {viewing.text}
            </pre>
          </div>
        )}
      </Card>

      <Card title="Messages" desc={`${messages.length} in manifest`}>
        <DataTable
          columns={msgColumns}
          rows={messages.map((m, i) => ({ ...m, _idx: i }))}
          rowKey={(r) => `${r._idx}`}
          empty="No per-message entries in this manifest."
          dense
        />
      </Card>
    </div>
  );
}

function TimeRow({ label, ms }: { label: string; ms?: number }) {
  return (
    <div className="flex items-center justify-between">
      <span className="text-muted-foreground">{label}</span>
      <span className="font-mono tabular-nums text-foreground">{fmtMs(ms)}</span>
    </div>
  );
}
