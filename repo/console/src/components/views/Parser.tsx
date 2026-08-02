import { useEffect, useMemo, useRef, useState } from "react";
import { cn } from "@/lib/utils";
import { PlaneHeader } from "@/components/PlaneHeader";
import { Segmented, Btn } from "@/components/setup/kit";
import { StatusBadge } from "@/components/StatusBadge";
import { TagChip, type TagTone } from "@/components/TagChip";
import { parseFin, type FinField, type FinMessage } from "@/lib/fin";
import { decodeChips, cashRecon, type CashRecon } from "@/lib/finDecode";
import { fmtSigned, fmtMs } from "@/lib/format";
import { logActivity } from "@/lib/activity";
import { toast } from "@/lib/toast";
import { submitUpload, pollJob, getManifest } from "@/lib/api";
import { errText } from "@/components/setup/kit";
import type { AppRoute } from "@/lib/route";
import type { MessageEntry, SchemaDef } from "@/lib/types";
import SAMPLES from "@/generated/samples.json";
import SCHEMAS from "@/generated/schemas.json";
import {
  ArrowRight,
  CheckCircle2,
  ChevronRight,
  FileWarning,
  Layers,
  ScanLine,
  Server,
  Upload,
  XCircle,
} from "lucide-react";

const samples = SAMPLES as { id: string; mt: string; label: string; category: string; fin: string }[];
const schemas = SCHEMAS as SchemaDef[];
const schemaByMt = new Map(schemas.map((s) => [s.mt, s]));

type ViewMode = "structured" | "raw" | "split";

interface ServerState {
  status: "idle" | "running" | "ok" | "fail";
  jobId?: string;
  entry?: MessageEntry;
  error?: string;
  ms?: number;
  jobError?: string | null;
}

export function Parser({ route, onRoute }: { route: Extract<AppRoute, { view: "parser" }>; onRoute: (r: AppRoute) => void }) {
  const [input, setInput] = useState("");
  const [parsed, setParsed] = useState<FinMessage | null>(null);
  const [view, setView] = useState<ViewMode>("split");
  const [activeIdx, setActiveIdx] = useState<number | null>(null);
  const [collapsed, setCollapsed] = useState(false);
  const [server, setServer] = useState<ServerState>({ status: "idle" });
  const rawRefs = useRef(new Map<number, HTMLElement>());

  // Deep-link: inspect a specific message of an existing job.
  useEffect(() => {
    if (!route.jobId) return;
    let live = true;
    getManifest(route.jobId)
      .then((m) => {
        if (!live) return;
        const idx = route.msgIndex ?? 0;
        const entry = m.messages?.[idx];
        setServer({ status: entry?.error ? "fail" : "ok", jobId: route.jobId, entry, jobError: null });
      })
      .catch((e) => live && setServer({ status: "fail", error: errText(e) }));
    return () => {
      live = false;
    };
  }, [route.jobId, route.msgIndex]);

  const doParse = (text = input) => {
    const trimmed = text.trim();
    if (!trimmed) return;
    const msg = parseFin(trimmed);
    setParsed(msg);
    setActiveIdx(null);
    setCollapsed(true);
    setServer({ status: "idle" });
    logActivity("Parse", `${msg.messageType ?? "unknown"} · ${msg.fields.length} fields`, msg.error ? "warn" : "ok");
  };

  const loadSample = (fin: string) => {
    setInput(fin);
    doParse(fin);
  };

  const onDrop = (e: React.DragEvent) => {
    e.preventDefault();
    const file = e.dataTransfer.files[0];
    if (!file) return;
    file.text().then((t) => {
      setInput(t);
      doParse(t);
    });
  };

  const validateOnServer = async () => {
    if (!parsed) return;
    setServer({ status: "running" });
    const t0 = performance.now();
    try {
      // swift-api's schema catalog is keyed with the "MT" prefix (e.g. "MT535",
      // "MT940") — send it verbatim; stripping it here never matched a schema.
      const sub = await submitUpload(input.trim(), parsed.messageType || undefined);
      const done = await pollJob(sub.job_id);
      const manifest = await getManifest(sub.job_id);
      const entry = manifest.messages?.[0];
      const ms = performance.now() - t0;
      setServer({ status: entry?.error ? "fail" : "ok", jobId: sub.job_id, entry, ms, jobError: done.error ?? null });
      toast(entry?.error ? "warn" : "ok", entry?.error ? "Server flagged an error" : "Validated on server", sub.job_id);
      logActivity("ServerValidate", `${sub.job_id} → ${done.status}`, entry?.error ? "warn" : "ok");
    } catch (e) {
      setServer({ status: "fail", error: errText(e) });
    }
  };

  const schema = parsed?.messageType ? schemaByMt.get(parsed.messageType) : undefined;
  const schemaTags = useMemo(() => new Set((schema?.fields ?? []).map((f) => f.tag.toUpperCase())), [schema]);
  const tagName = useMemo(() => {
    const m = new Map<string, string>();
    for (const f of schema?.fields ?? []) if (!m.has(f.tag.toUpperCase())) m.set(f.tag.toUpperCase(), f.name);
    return m;
  }, [schema]);
  const recon = useMemo(() => (parsed ? cashRecon(parsed.fields) : null), [parsed]);

  useEffect(() => {
    if (view !== "split" || activeIdx == null) return;
    rawRefs.current.get(activeIdx)?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [activeIdx, view]);

  return (
    <div className="flex h-full flex-col">
      <PlaneHeader
        plane="parser"
        planeLabel="Parser"
        route="swift-core → swift-schema"
        title="SWIFT parser workbench"
        summary="Paste, drop, or pick a FIN message. It is split into blocks and :NN: tag fields instantly, with a schema match and reconciliation read — offline. Validate on the server for the authoritative parse."
        actions={
          <div className="flex items-center gap-2">
            <Segmented
              value={view}
              onChange={(v) => setView(v)}
              options={[
                { id: "structured", label: "Structured" },
                { id: "split", label: "Split" },
                { id: "raw", label: "Raw" },
              ]}
            />
            {parsed && (
              <Btn onClick={() => setCollapsed((c) => !c)}>{collapsed ? "Edit input" : "Hide input"}</Btn>
            )}
          </div>
        }
      />

      <div className="flex min-h-0 flex-1">
        {/* Input / sample rail */}
        {(!parsed || !collapsed) && (
          <InputPanel
            input={input}
            setInput={setInput}
            onParse={() => doParse()}
            onDrop={onDrop}
            onSample={loadSample}
          />
        )}

        {parsed && (
          <>
            {/* Structure + raw */}
            <div className="min-w-0 flex-1 overflow-auto bg-surface-app p-4">
              {parsed.error && (
                <div className="mb-3 flex items-center gap-2 rounded-lg border border-warn/45 bg-warn/10 px-3 py-2 text-[12px] text-warn">
                  <FileWarning className="h-4 w-4 shrink-0" /> {parsed.error}
                </div>
              )}
              {recon && <ReconEquation recon={recon} />}

              {view !== "raw" && (
                <div className="mt-3 space-y-2.5">
                  {parsed.blocks.map((b) =>
                    b.id === "4" ? (
                      <Block4Card
                        key={b.id}
                        fields={parsed.fields}
                        schemaTags={schemaTags}
                        tagName={tagName}
                        activeIdx={activeIdx}
                        onHover={setActiveIdx}
                      />
                    ) : (
                      <HeaderBlockCard key={b.id} label={b.label} id={b.id} kv={b.kv} raw={b.raw} />
                    ),
                  )}
                </div>
              )}

              {view === "raw" && (
                <RawView msg={parsed} activeIdx={activeIdx} onHover={setActiveIdx} rawRefs={rawRefs} />
              )}
              {view === "split" && (
                <div className="mt-3">
                  <div className="mb-1.5 text-[10.5px] font-semibold uppercase tracking-wider text-muted-foreground">
                    Raw source
                  </div>
                  <RawView msg={parsed} activeIdx={activeIdx} onHover={setActiveIdx} rawRefs={rawRefs} />
                </div>
              )}
            </div>

            {/* Verdict */}
            <VerdictPanel
              msg={parsed}
              schema={schema}
              schemaTags={schemaTags}
              recon={recon}
              server={server}
              onValidate={validateOnServer}
              onRoute={onRoute}
            />
          </>
        )}

        {!parsed && (
          <div className="flex flex-1 items-center justify-center p-8">
            <div className="max-w-sm text-center">
              <ScanLine className="mx-auto h-8 w-8 text-plane-parser/70" />
              <p className="mt-3 text-[14px] font-semibold">Parse a SWIFT message</p>
              <p className="mt-1 text-[12.5px] text-muted-foreground">
                Pick a sample on the left, paste a FIN message, or drop a <code className="font-mono">.fin</code> file. It is
                dissected block by block, tag by tag.
              </p>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
function InputPanel({
  input,
  setInput,
  onParse,
  onDrop,
  onSample,
}: {
  input: string;
  setInput: (v: string) => void;
  onParse: () => void;
  onDrop: (e: React.DragEvent) => void;
  onSample: (fin: string) => void;
}) {
  const [q, setQ] = useState("");
  const filtered = samples.filter((s) => s.mt.toLowerCase().includes(q.toLowerCase()) || s.category.includes(q.toLowerCase()));
  return (
    <div className="flex w-[340px] shrink-0 flex-col border-r border-outline-subtle bg-surface-sidebar">
      <div className="flex flex-col gap-2 border-b border-outline-subtle p-3">
        <textarea
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onDrop={onDrop}
          onDragOver={(e) => e.preventDefault()}
          spellCheck={false}
          placeholder={"{1:F01BANKGB22AXXX0000000000}{2:I940BANKDEFFXXXXN}{4:\n:20:...\n-}"}
          className="h-40 w-full resize-none rounded-md border border-outline-strong bg-surface-input p-2.5 font-mono text-[11.5px] leading-relaxed text-foreground/90 outline-none placeholder:text-muted-foreground/40 focus:border-plane-parser/50"
        />
        <div className="flex items-center gap-2">
          <Btn variant="primary" onClick={onParse} className="inline-flex items-center gap-1.5">
            <ScanLine className="h-3.5 w-3.5" /> Parse
          </Btn>
          <span className="inline-flex items-center gap-1 text-[11px] text-muted-foreground">
            <Upload className="h-3 w-3" /> or drop a .fin file
          </span>
        </div>
      </div>
      <div className="flex items-center gap-2 px-3 pb-2 pt-3">
        <span className="text-[10.5px] font-semibold uppercase tracking-wider text-muted-foreground">Sample library</span>
        <span className="rounded bg-surface-toolbar px-1.5 py-0.5 font-mono text-[10px] text-muted-foreground">{samples.length}</span>
      </div>
      <div className="px-3 pb-2">
        <input
          value={q}
          onChange={(e) => setQ(e.target.value)}
          placeholder="Filter MT type…"
          className="w-full rounded-md border border-outline-strong bg-surface-input px-2 py-1 text-[11.5px] outline-none focus:border-plane-parser/50"
        />
      </div>
      <div className="min-h-0 flex-1 overflow-auto px-3 pb-3">
        <div className="grid grid-cols-2 gap-1.5">
          {filtered.map((s) => (
            <button
              key={s.id}
              type="button"
              onClick={() => onSample(s.fin)}
              className="flex flex-col items-start gap-0.5 rounded-md border border-outline-subtle bg-surface-panel px-2 py-1.5 text-left transition-colors hover:border-plane-parser/40 hover:bg-surface-hover"
            >
              <span className="font-mono text-[11.5px] font-semibold text-foreground">{s.label}</span>
              <span className="truncate text-[10px] capitalize text-muted-foreground">{s.category}</span>
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
function ReconEquation({ recon }: { recon: CashRecon }) {
  const n = (x: number) => x.toLocaleString("en-US", { minimumFractionDigits: 2 });
  return (
    <div
      className={cn(
        "flex flex-wrap items-center gap-x-2 gap-y-1 rounded-lg border px-3.5 py-2.5 font-mono text-[12.5px]",
        recon.reconciles ? "border-ok/40 bg-ok/8" : "border-destructive/45 bg-destructive/8",
      )}
    >
      <span className="text-muted-foreground">opening</span>
      <span className="tabular-nums text-foreground">{n(recon.opening)}</span>
      <span className="text-muted-foreground">+</span>
      <span className="text-muted-foreground">Σ {recon.entryCount} entries</span>
      <span className={cn("tabular-nums", recon.net >= 0 ? "text-credit" : "text-debit")}>({fmtSigned(recon.net)})</span>
      <span className="text-muted-foreground">=</span>
      <span className="tabular-nums text-foreground">{n(recon.closing)}</span>
      <span className="ml-auto inline-flex items-center gap-1.5">
        {recon.reconciles ? (
          <StatusBadge variant="ok">reconciles</StatusBadge>
        ) : (
          <StatusBadge variant="error">break {fmtSigned(recon.diff)}</StatusBadge>
        )}
        <span className="text-[11px] text-muted-foreground">{recon.currency}</span>
      </span>
    </div>
  );
}

// ---------------------------------------------------------------------------
function HeaderBlockCard({ label, id, kv, raw }: { label: string; id: string; kv?: { label: string; value: string; hint?: string }[]; raw: string }) {
  return (
    <section className="overflow-hidden rounded-lg border border-outline-subtle bg-surface-panel">
      <div className="flex items-center gap-2 border-b border-outline-subtle/70 bg-surface-toolbar/50 px-3 py-1.5">
        <span className="grid h-5 w-5 place-items-center rounded bg-surface-active font-mono text-[10px] font-semibold text-muted-foreground">{id}</span>
        <span className="text-[12px] font-medium text-foreground">{label}</span>
      </div>
      <div className="px-3 py-2">
        {kv?.length ? (
          <div className="grid grid-cols-2 gap-x-6 gap-y-1.5 sm:grid-cols-3">
            {kv.map((k) => (
              <div key={k.label} className="min-w-0">
                <div className="text-[10px] uppercase tracking-wider text-muted-foreground">{k.label}</div>
                <div className="truncate font-mono text-[12px] text-foreground">{k.value || "—"}</div>
                {k.hint && <div className="truncate text-[10px] text-muted-foreground">{k.hint}</div>}
              </div>
            ))}
          </div>
        ) : (
          <code className="block whitespace-pre-wrap break-all font-mono text-[11px] text-muted-foreground">{raw || "(empty)"}</code>
        )}
      </div>
    </section>
  );
}

// ---------------------------------------------------------------------------
function Block4Card({
  fields,
  schemaTags,
  tagName,
  activeIdx,
  onHover,
}: {
  fields: FinField[];
  schemaTags: Set<string>;
  tagName: Map<string, string>;
  activeIdx: number | null;
  onHover: (i: number | null) => void;
}) {
  return (
    <section className="overflow-hidden rounded-lg border border-outline-subtle bg-surface-panel">
      <div className="flex items-center gap-2 border-b border-outline-subtle/70 bg-surface-toolbar/50 px-3 py-1.5">
        <span className="grid h-5 w-5 place-items-center rounded bg-plane-parser/20 font-mono text-[10px] font-semibold text-plane-parser">4</span>
        <span className="text-[12px] font-medium text-foreground">Text Block</span>
        <span className="ml-auto text-[10.5px] text-muted-foreground">{fields.filter((f) => f.kind === "field").length} fields</span>
      </div>
      <div className="divide-y divide-outline-subtle/40">
        {fields.map((f, i) => (
          <FieldRow
            key={i}
            field={f}
            idx={i}
            inSchema={schemaTags.size === 0 || schemaTags.has(f.tag.toUpperCase()) || f.kind !== "field"}
            name={tagName.get(f.tag.toUpperCase())}
            active={activeIdx === i}
            onHover={onHover}
          />
        ))}
      </div>
    </section>
  );
}

function FieldRow({
  field,
  idx,
  inSchema,
  name,
  active,
  onHover,
}: {
  field: FinField;
  idx: number;
  inSchema: boolean;
  name?: string;
  active: boolean;
  onHover: (i: number | null) => void;
}) {
  const indent = field.seqPath.length * 14;
  const chips = field.kind === "field" ? decodeChips(field) : [];

  if (field.kind === "seqStart") {
    return (
      <div
        onMouseEnter={() => onHover(idx)}
        onMouseLeave={() => onHover(null)}
        style={{ paddingLeft: 12 + indent }}
        className={cn("flex items-center gap-1.5 py-1 pr-3", active && "bg-surface-hover")}
      >
        <ChevronRight className="h-3 w-3 text-plane-control" />
        <span className="text-[11px] font-semibold uppercase tracking-wide text-plane-control">{field.seqName}</span>
        <span className="font-mono text-[9.5px] text-muted-foreground">:16R:</span>
      </div>
    );
  }
  if (field.kind === "seqEnd") return null;

  const tone: TagTone = inSchema ? "active" : "warn";
  return (
    <div
      onMouseEnter={() => onHover(idx)}
      onMouseLeave={() => onHover(null)}
      style={{ paddingLeft: 12 + indent }}
      className={cn(
        "group flex flex-col gap-1 py-1.5 pr-3 transition-colors",
        active ? "bg-plane-parser/8" : "hover:bg-surface-hover",
        !inSchema && "bg-warn/[0.03]",
      )}
    >
      <div className="flex items-center gap-2">
        <TagChip tag={field.tag} tone={tone} />
        {name ? (
          <span className="text-[12px] font-medium text-foreground">{name.replace(/_/g, " ")}</span>
        ) : (
          <span className="text-[12px] text-muted-foreground">{inSchema ? "field" : "not in schema"}</span>
        )}
        {chips.map((c, ci) => (
          <span
            key={ci}
            className={cn(
              "inline-flex items-center gap-1 rounded border border-outline-subtle bg-surface-app px-1.5 py-0.5 text-[10.5px]",
              c.tone === "credit" && "border-credit/40 text-credit",
              c.tone === "debit" && "border-debit/40 text-debit",
            )}
          >
            <span className="text-muted-foreground">{c.label}</span>
            <span className="font-mono tabular-nums">{c.value}</span>
          </span>
        ))}
      </div>
      <code className="whitespace-pre-wrap break-all font-mono text-[11px] leading-snug text-muted-foreground/80">{field.value}</code>
    </div>
  );
}

// ---------------------------------------------------------------------------
function RawView({
  msg,
  activeIdx,
  onHover,
  rawRefs,
}: {
  msg: FinMessage;
  activeIdx: number | null;
  onHover: (i: number | null) => void;
  rawRefs: React.MutableRefObject<Map<number, HTMLElement>>;
}) {
  // Segment the raw source at field spans so hovering a run syncs the tree.
  const segments: React.ReactNode[] = [];
  let cursor = 0;
  const raw = msg.raw;
  msg.fields.forEach((f, i) => {
    if (f.span.start > cursor) segments.push(<span key={`t${i}`}>{raw.slice(cursor, f.span.start)}</span>);
    segments.push(
      <span
        key={`f${i}`}
        ref={(el) => {
          if (el) rawRefs.current.set(i, el);
        }}
        onMouseEnter={() => onHover(i)}
        onMouseLeave={() => onHover(null)}
        className={cn(
          "rounded-sm",
          activeIdx === i ? "bg-plane-parser/30 text-foreground" : "hover:bg-surface-active",
          f.kind !== "field" && "text-plane-control",
        )}
      >
        {raw.slice(f.span.start, f.span.end)}
      </span>,
    );
    cursor = f.span.end;
  });
  if (cursor < raw.length) segments.push(<span key="tail">{raw.slice(cursor)}</span>);

  return (
    <pre className="overflow-auto rounded-lg border border-outline-subtle bg-surface-input p-3 font-mono text-[11.5px] leading-relaxed text-foreground/85">
      {segments}
    </pre>
  );
}

// ---------------------------------------------------------------------------
function VerdictPanel({
  msg,
  schema,
  schemaTags,
  recon,
  server,
  onValidate,
  onRoute,
}: {
  msg: FinMessage;
  schema?: SchemaDef;
  schemaTags: Set<string>;
  recon: CashRecon | null;
  server: ServerState;
  onValidate: () => void;
  onRoute: (r: AppRoute) => void;
}) {
  const fieldTags = msg.fields.filter((f) => f.kind === "field");
  const matched = fieldTags.filter((f) => schemaTags.has(f.tag.toUpperCase())).length;
  const unknown = schemaTags.size ? fieldTags.filter((f) => !schemaTags.has(f.tag.toUpperCase())) : [];

  return (
    <aside className="flex w-[320px] shrink-0 flex-col gap-3 overflow-auto border-l border-outline-subtle bg-surface-sidebar p-3">
      {/* Schema match */}
      <Panel icon={Layers} title="Schema match" accent="control">
        {msg.messageType ? (
          schema ? (
            <>
              <div className="flex items-center gap-2">
                <span className="font-mono text-[13px] font-semibold text-foreground">{msg.messageType}</span>
                <StatusBadge variant="ok">matched</StatusBadge>
              </div>
              <p className="mt-1 text-[11px] text-muted-foreground">
                {schema.category} · {schema.sequences.length} sequences · matched by block-2 type.
              </p>
              <div className="mt-2 flex items-center justify-between text-[11.5px]">
                <span className="text-muted-foreground">tag coverage</span>
                <span className="font-mono tabular-nums">
                  {matched}/{fieldTags.length}
                </span>
              </div>
              <button
                type="button"
                onClick={() => onRoute({ view: "schemas", mt: msg.messageType! })}
                className="mt-2 inline-flex items-center gap-1 text-[11.5px] font-medium text-plane-control hover:underline"
              >
                Open {msg.messageType} schema <ArrowRight className="h-3 w-3" />
              </button>
            </>
          ) : (
            <div className="flex items-center gap-2">
              <span className="font-mono text-[13px] font-semibold text-foreground">{msg.messageType}</span>
              <StatusBadge variant="warn">no schema</StatusBadge>
            </div>
          )
        ) : (
          <StatusBadge variant="warn">message type not detected</StatusBadge>
        )}
      </Panel>

      {/* Validation */}
      <Panel icon={ScanLine} title="Validation" accent="parser">
        {unknown.length ? (
          <>
            <StatusBadge variant="warn">{unknown.length} tag(s) not in schema</StatusBadge>
            <div className="mt-2 flex flex-wrap gap-1">
              {unknown.map((f, i) => (
                <TagChip key={i} tag={f.tag} tone="warn" />
              ))}
            </div>
          </>
        ) : schemaTags.size ? (
          <StatusBadge variant="ok">all tags recognised</StatusBadge>
        ) : (
          <span className="text-[11.5px] text-muted-foreground">Structural parse only — no schema loaded for this type.</span>
        )}
        <p className="mt-2 text-[10.5px] leading-snug text-muted-foreground/80">
          Structural check only. Run the server for swift-core / swift-schema's authoritative validation.
        </p>
      </Panel>

      {/* Normalized output */}
      <Panel icon={Layers} title="Normalized read" accent="books">
        {recon ? (
          <div className="space-y-1 text-[11.5px]">
            <Row label="Statement" value="1 cash statement" />
            <Row label="Entries" value={`${recon.entryCount}`} />
            <Row label="Reconciles" value={recon.reconciles ? "yes ✓" : "no ✗"} tone={recon.reconciles ? "ok" : "error"} />
            <button
              type="button"
              onClick={() => onRoute({ view: "recon" })}
              className="mt-1 inline-flex items-center gap-1 text-[11.5px] font-medium text-plane-books hover:underline"
            >
              Open Reconciliation <ArrowRight className="h-3 w-3" />
            </button>
          </div>
        ) : msg.fields.some((f) => f.tag === "35B") ? (
          <div className="space-y-1 text-[11.5px]">
            <Row label="Positions" value={`${msg.fields.filter((f) => f.tag === "35B").length} instrument(s)`} />
            <button
              type="button"
              onClick={() => onRoute({ view: "positions" })}
              className="mt-1 inline-flex items-center gap-1 text-[11.5px] font-medium text-plane-books hover:underline"
            >
              Open Positions <ArrowRight className="h-3 w-3" />
            </button>
          </div>
        ) : (
          <span className="text-[11.5px] text-muted-foreground">No cash/position read derived from this message.</span>
        )}
      </Panel>

      {/* Server validation */}
      <Panel icon={Server} title="Server parse" accent="pipeline">
        {server.status === "idle" && (
          <>
            <p className="text-[11.5px] text-muted-foreground">Submit to swift-api for the authoritative parse, render and error report.</p>
            <Btn onClick={onValidate} className="mt-2 inline-flex items-center gap-1.5">
              <Server className="h-3.5 w-3.5" /> Validate on server
            </Btn>
          </>
        )}
        {server.status === "running" && <StatusBadge variant="info" pulse>submitting…</StatusBadge>}
        {server.status === "ok" && (
          <div className="space-y-1.5">
            <div className="flex items-center gap-1.5 text-[12px] text-ok">
              <CheckCircle2 className="h-4 w-4" /> parsed cleanly {server.ms != null && <span className="text-muted-foreground">· {fmtMs(server.ms)}</span>}
            </div>
            {server.jobId && (
              <button type="button" onClick={() => onRoute({ view: "jobs", jobId: server.jobId })} className="inline-flex items-center gap-1 text-[11.5px] font-medium text-plane-pipeline hover:underline">
                Open job {server.jobId.slice(0, 8)} <ArrowRight className="h-3 w-3" />
              </button>
            )}
          </div>
        )}
        {server.status === "fail" && (
          <div className="space-y-1.5">
            <div className="flex items-start gap-1.5 text-[12px] text-destructive">
              <XCircle className="mt-0.5 h-4 w-4 shrink-0" />
              <span>{server.entry?.error ?? server.error ?? "Server parse failed"}</span>
            </div>
            <Btn onClick={onValidate}>Retry</Btn>
          </div>
        )}
      </Panel>
    </aside>
  );
}

function Panel({ icon: Icon, title, accent, children }: { icon: typeof Layers; title: string; accent: "parser" | "control" | "books" | "pipeline"; children: React.ReactNode }) {
  const tone = { parser: "text-plane-parser", control: "text-plane-control", books: "text-plane-books", pipeline: "text-plane-pipeline" }[accent];
  return (
    <section className="rounded-lg border border-outline-subtle bg-surface-panel p-3">
      <div className="mb-2 flex items-center gap-1.5">
        <Icon className={cn("h-3.5 w-3.5", tone)} />
        <span className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">{title}</span>
      </div>
      {children}
    </section>
  );
}

function Row({ label, value, tone }: { label: string; value: string; tone?: "ok" | "error" }) {
  return (
    <div className="flex items-center justify-between">
      <span className="text-muted-foreground">{label}</span>
      <span className={cn("font-mono tabular-nums", tone === "ok" && "text-ok", tone === "error" && "text-destructive")}>{value}</span>
    </div>
  );
}
