// Control plane — provenance / source inventory over the recon read-model
// (GET /recon/snapshot). ingest-core is source-agnostic: it consumes only
// normalized SecurityEvent/CashStatement records, and SWIFT FIN is just one
// of several adapters that produce them.
import { AlertTriangle, RefreshCw, Waypoints } from "lucide-react";
import { PlaneHeader } from "@/components/PlaneHeader";
import { Card, Btn } from "@/components/setup/kit";
import { DataTable, type Column } from "@/components/DataTable";
import { TagChip } from "@/components/TagChip";
import { StatusBadge } from "@/components/StatusBadge";
import { PillarEmpty } from "@/components/PillarEmpty";
import { useSnapshot } from "@/lib/useSnapshot";
import { fmtInt, relTime } from "@/lib/format";
import type { AppRoute } from "@/lib/route";
import type { SchemaDef } from "@/lib/types";
import SCHEMAS from "@/generated/schemas.json";

const schemas = SCHEMAS as SchemaDef[];
const schemaByMt = new Map(schemas.map((s) => [s.mt, s]));

const ADAPTERS: { id: string; label: string; note: string }[] = [
  { id: "swift-normalize", label: "SWIFT FIN", note: "MT category messages (cash, securities, FX) normalized off the wire format." },
  { id: "ingest-tabular", label: "Tabular custodian CSV", note: "Custodian statement/position extracts mapped onto the same normalized shape." },
  { id: "swift-mt940", label: "MT940 cash", note: "Cash statement/turnover messages decoded into SecurityEvent/CashStatement." },
  { id: "mx-camt", label: "camt.053 MX / ISO 20022", note: "XML cash management messages, the ISO 20022 successor to MT940." },
];

/** "MT535" -> "535"; undefined when the message type isn't a SWIFT MT tag. */
function mtTag(mt: string): string | undefined {
  const m = mt.match(/^MT(\d{3})$/i);
  return m ? m[1] : undefined;
}

interface SourceAgg {
  source: string;
  statements: number;
  positions: number;
  messageTypes: Set<string>;
}

interface MessageTypeRow {
  messageType: string;
  category: string;
  hasSchema: boolean;
  count: number;
}

export function Sources({ onRoute }: { onRoute: (r: AppRoute) => void }) {
  const { result, error, at, refresh } = useSnapshot();
  const snapshot = result?.snapshot;

  const refreshBtn = (
    <Btn variant="ghost" onClick={refresh} className="inline-flex items-center gap-1.5">
      <RefreshCw className="h-3.5 w-3.5" /> Refresh
    </Btn>
  );

  const statements = snapshot?.statements ?? [];
  const positions = snapshot?.positions ?? [];

  const bySource = new Map<string, SourceAgg>();
  const bump = (source: string): SourceAgg => {
    let agg = bySource.get(source);
    if (!agg) {
      agg = { source, statements: 0, positions: 0, messageTypes: new Set() };
      bySource.set(source, agg);
    }
    return agg;
  };
  for (const s of statements) {
    const agg = bump(s.source);
    agg.statements += 1;
    agg.messageTypes.add(s.messageType);
  }
  for (const p of positions) {
    const agg = bump(p.source);
    agg.positions += 1;
    agg.messageTypes.add(p.messageType);
  }
  const sources = [...bySource.values()].sort((a, b) => a.source.localeCompare(b.source));

  const mtCounts = new Map<string, number>();
  for (const s of statements) mtCounts.set(s.messageType, (mtCounts.get(s.messageType) ?? 0) + 1);
  for (const p of positions) mtCounts.set(p.messageType, (mtCounts.get(p.messageType) ?? 0) + 1);

  const messageTypeRows: MessageTypeRow[] = [...mtCounts.entries()]
    .map(([messageType, count]) => {
      const schema = schemaByMt.get(messageType);
      return {
        messageType,
        category: schema?.category ?? "—",
        hasSchema: Boolean(schema),
        count,
      };
    })
    .sort((a, b) => b.count - a.count);

  const columns: Column<MessageTypeRow>[] = [
    {
      key: "type",
      header: "Type",
      mono: true,
      sortValue: (r) => r.messageType,
      render: (r) => {
        const tag = mtTag(r.messageType);
        return tag ? <TagChip tag={tag} /> : <span className="font-mono text-[11.5px]">{r.messageType}</span>;
      },
    },
    {
      key: "category",
      header: "Category",
      sortValue: (r) => r.category,
      render: (r) => (r.category === "—" ? <span className="text-muted-foreground">—</span> : <span className="capitalize">{r.category}</span>),
    },
    {
      key: "schema",
      header: "Schema",
      sortValue: (r) => (r.hasSchema ? 1 : 0),
      render: (r) => (r.hasSchema ? <StatusBadge variant="ok">schema</StatusBadge> : <StatusBadge variant="neutral">none</StatusBadge>),
    },
    {
      key: "count",
      header: "Count",
      align: "right",
      mono: true,
      sortValue: (r) => r.count,
      render: (r) => fmtInt(r.count),
    },
  ];

  const empty = statements.length === 0 && positions.length === 0;

  return (
    <div className="flex h-full flex-col">
      <PlaneHeader
        plane="control"
        planeLabel="Control"
        route="ingest-core"
        title="Sources"
        summary="Where data comes from — provenance and message types seen across the recon store."
        actions={refreshBtn}
      />
      <div className="min-h-0 flex-1 overflow-auto p-5 space-y-4">
        {result?.stale && (
          <div className="flex items-center gap-2 rounded-lg border border-warn/45 bg-warn/10 px-3 py-2 text-[12px] text-warn">
            <AlertTriangle className="h-4 w-4 shrink-0" />
            Offline snapshot · as of {relTime(at)}
          </div>
        )}
        {error && !result && (
          <div className="flex items-center gap-2 rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-[12px] text-destructive">
            <AlertTriangle className="h-4 w-4 shrink-0" /> {error}
          </div>
        )}

        <Card title="Adapters" desc="ingest-core consumes only normalized SecurityEvent/CashStatement — SWIFT is just one source.">
          <div className="space-y-1.5">
            {ADAPTERS.map((a) => (
              <div key={a.id} className="flex items-center gap-3 rounded-md border border-outline-subtle bg-surface-panel px-3 py-1.5">
                <code className="shrink-0 rounded border border-outline-strong bg-surface-toolbar px-1.5 py-0.5 font-mono text-[10.5px] text-muted-foreground">
                  {a.id}
                </code>
                <span className="shrink-0 text-[12px] font-medium text-foreground">{a.label}</span>
                <span className="min-w-0 truncate text-[11.5px] text-muted-foreground">{a.note}</span>
              </div>
            ))}
          </div>
        </Card>

        {empty ? (
          <PillarEmpty
            icon={Waypoints}
            title="No sources yet"
            body="No statements or positions have been ingested yet — provenance appears here once the recon store has data."
          />
        ) : (
          <>
            <div className="grid grid-cols-3 gap-3">
              {sources.map((s) => (
                <div key={s.source} className="rounded-lg border border-outline-subtle bg-surface-panel p-3.5">
                  <code className="inline-block truncate rounded border border-outline-strong bg-surface-toolbar px-1.5 py-0.5 font-mono text-[11px] font-semibold text-foreground">
                    {s.source}
                  </code>
                  <div className="mt-2 space-y-1 text-[12px]">
                    <div className="flex items-center justify-between">
                      <span className="text-muted-foreground">Statements</span>
                      <span className="font-mono tabular-nums text-foreground">{fmtInt(s.statements)}</span>
                    </div>
                    <div className="flex items-center justify-between">
                      <span className="text-muted-foreground">Positions</span>
                      <span className="font-mono tabular-nums text-foreground">{fmtInt(s.positions)}</span>
                    </div>
                    <div className="flex items-center justify-between">
                      <span className="text-muted-foreground">Message types</span>
                      <span className="font-mono tabular-nums text-foreground">{fmtInt(s.messageTypes.size)}</span>
                    </div>
                  </div>
                </div>
              ))}
            </div>

            <Card title="Message types seen" desc={`${messageTypeRows.length} distinct type${messageTypeRows.length === 1 ? "" : "s"} across statements and positions`}>
              <DataTable
                columns={columns}
                rows={messageTypeRows}
                rowKey={(r) => r.messageType}
                onRowClick={(r) => {
                  if (r.hasSchema) onRoute({ view: "schemas", mt: r.messageType });
                }}
                empty="No message types seen."
              />
            </Card>
          </>
        )}
      </div>
    </div>
  );
}
