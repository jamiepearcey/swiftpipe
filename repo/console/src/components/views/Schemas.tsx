// Schema catalog browser — the YAML schemas that drive parse, validate and
// render for every MT message type swiftpipe understands. Read-only: this is
// a map of the terrain, not an editor.
import { useMemo, useState } from "react";
import { PlaneHeader } from "@/components/PlaneHeader";
import { DataTable, type Column } from "@/components/DataTable";
import { StatusBadge } from "@/components/StatusBadge";
import { TagChip } from "@/components/TagChip";
import { Btn, Card, inputCls } from "@/components/setup/kit";
import { PillarEmpty } from "@/components/PillarEmpty";
import { cn } from "@/lib/utils";
import type { AppRoute } from "@/lib/route";
import type { SampleDef, SchemaDef, SchemaField } from "@/lib/types";
import SCHEMAS from "@/generated/schemas.json";
import SAMPLES from "@/generated/samples.json";
import { BookOpen, Search, ScanLine, ShieldAlert, ShieldCheck } from "lucide-react";

const schemas = SCHEMAS as SchemaDef[];
const samples = SAMPLES as SampleDef[];

export function Schemas({
  route,
  onRoute,
}: {
  route: Extract<AppRoute, { view: "schemas" }>;
  onRoute: (r: AppRoute) => void;
}) {
  const [filter, setFilter] = useState("");

  const selectedMt = route.mt ?? schemas[0]?.mt;
  const schema = schemas.find((s) => s.mt === selectedMt);
  const hasSample = samples.some((s) => s.mt === selectedMt);

  const filtered = useMemo(() => {
    const q = filter.trim().toLowerCase();
    if (!q) return schemas;
    return schemas.filter((s) => s.mt.toLowerCase().includes(q) || s.category.toLowerCase().includes(q));
  }, [filter]);

  const groups = useMemo(() => {
    const byCategory = new Map<string, SchemaDef[]>();
    for (const s of filtered) {
      const g = byCategory.get(s.category) ?? [];
      g.push(s);
      byCategory.set(s.category, g);
    }
    for (const g of byCategory.values()) g.sort((a, b) => a.mt.localeCompare(b.mt));
    return [...byCategory.entries()].sort((a, b) => a[0].localeCompare(b[0]));
  }, [filtered]);

  const fieldColumns: Column<SchemaField>[] = [
    {
      key: "path",
      header: "Path",
      mono: true,
      render: (f) => <span className="text-muted-foreground">{f.path}</span>,
      sortValue: (f) => f.path,
    },
    {
      key: "tag",
      header: "Tag",
      render: (f) => <TagChip tag={f.tag} />,
      sortValue: (f) => f.tag,
    },
    {
      key: "name",
      header: "Name",
      render: (f) => f.name.replace(/_/g, " "),
      sortValue: (f) => f.name,
    },
    {
      key: "qualifier",
      header: "Qualifier",
      mono: true,
      render: (f) => f.qualifier ?? <span className="text-muted-foreground">—</span>,
      sortValue: (f) => f.qualifier ?? "",
    },
    {
      key: "required",
      header: "Req",
      render: (f) =>
        f.required ? (
          <StatusBadge variant="active">req</StatusBadge>
        ) : (
          <span className="text-muted-foreground">opt</span>
        ),
      sortValue: (f) => (f.required ? 1 : 0),
    },
    {
      key: "type",
      header: "Type",
      render: (f) => f.type,
      sortValue: (f) => f.type,
    },
    {
      key: "entity",
      header: "Entity / column",
      mono: true,
      render: (f) => (
        <span className="text-muted-foreground">
          {f.entity} · {f.column}
        </span>
      ),
      sortValue: (f) => `${f.entity}.${f.column}`,
    },
  ];

  if (!schemas.length) {
    return (
      <div className="flex h-full flex-col">
        <PlaneHeader
          plane="control"
          planeLabel="Control"
          route="examples/schemas"
          title="Schema catalog"
          summary="The YAML schemas that drive parse, validate and render — one per MT message type."
        />
        <div className="min-h-0 flex-1 overflow-auto p-5">
          <PillarEmpty
            icon={BookOpen}
            title="No schemas found"
            body="The generated schema catalog is empty — check scripts/gen-assets.mjs and examples/schemas."
          />
        </div>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col">
      <PlaneHeader
        plane="control"
        planeLabel="Control"
        route="examples/schemas"
        title="Schema catalog"
        summary="The YAML schemas that drive parse, validate and render — one per MT message type."
      />

      <div className="min-h-0 flex-1 overflow-auto p-5">
        <div className="flex items-start gap-4">
          {/* LEFT: filterable, grouped list */}
          <aside className="sticky top-0 flex max-h-[calc(100vh-160px)] w-72 shrink-0 flex-col overflow-hidden rounded-lg border border-outline-subtle bg-surface-panel">
            <div className="shrink-0 border-b border-outline-subtle p-2">
              <div className="relative">
                <Search className="pointer-events-none absolute left-2 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
                <input
                  value={filter}
                  onChange={(e) => setFilter(e.target.value)}
                  placeholder="Filter by MT or category…"
                  className={cn(inputCls, "pl-7")}
                  aria-label="Filter schemas"
                />
              </div>
            </div>
            <div className="min-h-0 flex-1 overflow-auto p-2">
              {groups.length ? (
                <div className="space-y-3">
                  {groups.map(([category, items]) => (
                    <div key={category}>
                      <div className="px-2 pb-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
                        {category} <span className="tabular-nums">({items.length})</span>
                      </div>
                      <div className="space-y-0.5">
                        {items.map((s) => {
                          const active = s.mt === selectedMt;
                          return (
                            <button
                              key={s.mt}
                              type="button"
                              onClick={() => onRoute({ view: "schemas", mt: s.mt })}
                              className={cn(
                                "flex w-full items-center justify-between rounded px-2 py-1.5 text-left transition-colors",
                                active ? "bg-plane-control/15 text-plane-control" : "hover:bg-surface-hover",
                              )}
                            >
                              <span className={cn("font-mono text-[12px] font-semibold", !active && "text-foreground")}>
                                {s.mt}
                              </span>
                              <span className="truncate text-[10.5px] text-muted-foreground">{s.category}</span>
                            </button>
                          );
                        })}
                      </div>
                    </div>
                  ))}
                </div>
              ) : (
                <div className="p-4 text-center text-[12px] text-muted-foreground">
                  No schemas match &ldquo;{filter}&rdquo;.
                </div>
              )}
            </div>
          </aside>

          {/* RIGHT: schema detail */}
          <div className="min-w-0 flex-1 space-y-4">
            {!schema ? (
              <PillarEmpty
                icon={BookOpen}
                title="No schema selected"
                body="Pick an MT message type from the list to see its schema."
              />
            ) : (
              <>
                <Card>
                  <div className="flex flex-wrap items-center gap-3">
                    <span className="font-mono text-[18px] font-semibold text-foreground">{schema.mt}</span>
                    <StatusBadge variant="neutral">{schema.version}</StatusBadge>
                    <span className="text-[12px] text-muted-foreground">{schema.category}</span>
                    {hasSample && (
                      <Btn
                        variant="primary"
                        className="ml-auto inline-flex items-center gap-1.5"
                        onClick={() => onRoute({ view: "parser" })}
                      >
                        <ScanLine className="h-3.5 w-3.5" /> Open in Parser
                      </Btn>
                    )}
                  </div>
                </Card>

                {schema.coverage && (
                  <Card title="Coverage">
                    <div className="flex flex-wrap items-center gap-3">
                      {schema.coverage.exact ? (
                        <StatusBadge variant="ok">
                          <ShieldCheck className="h-3 w-3" /> certified — exact match
                        </StatusBadge>
                      ) : (
                        <StatusBadge variant="warn">
                          <ShieldAlert className="h-3 w-3" /> starter — not certified
                        </StatusBadge>
                      )}
                      {schema.coverage.source && (
                        <span className="text-[11.5px] italic text-muted-foreground">{schema.coverage.source}</span>
                      )}
                    </div>
                    <div className="mt-3 grid grid-cols-2 gap-3 sm:w-64">
                      <div>
                        <div className="text-[10.5px] uppercase tracking-wider text-muted-foreground">
                          Expected sequences
                        </div>
                        <div className="font-mono text-[13px] tabular-nums text-foreground">
                          {schema.coverage.expected_sequences ?? "—"}
                          <span className="ml-1 text-[11px] text-muted-foreground">
                            / {schema.sequences.length} modeled
                          </span>
                        </div>
                      </div>
                      <div>
                        <div className="text-[10.5px] uppercase tracking-wider text-muted-foreground">
                          Expected fields
                        </div>
                        <div className="font-mono text-[13px] tabular-nums text-foreground">
                          {schema.coverage.expected_fields ?? "—"}
                          <span className="ml-1 text-[11px] text-muted-foreground">
                            / {schema.fields.length} modeled
                          </span>
                        </div>
                      </div>
                    </div>
                    {schema.coverage.notes && schema.coverage.notes.length > 0 && (
                      <ul className="mt-3 list-disc space-y-1 pl-4 text-[11.5px] leading-snug text-muted-foreground">
                        {schema.coverage.notes.map((n, i) => (
                          <li key={i}>{n}</li>
                        ))}
                      </ul>
                    )}
                  </Card>
                )}

                <Card title="Sequences" desc={`${schema.sequences.length} block${schema.sequences.length === 1 ? "" : "s"}`}>
                  {schema.sequences.length ? (
                    <div className="flex flex-wrap gap-1.5">
                      {schema.sequences.map((seq) => (
                        <span
                          key={seq}
                          className="rounded border border-outline-strong bg-surface-toolbar px-1.5 py-0.5 font-mono text-[10.5px] text-foreground/85"
                        >
                          {seq}
                        </span>
                      ))}
                    </div>
                  ) : (
                    <div className="text-[12px] text-muted-foreground">No sequences modeled for this schema.</div>
                  )}
                </Card>

                <section>
                  <h2 className="mb-2 text-[10.5px] font-semibold uppercase tracking-wider text-muted-foreground">
                    Fields ({schema.fields.length})
                  </h2>
                  <DataTable
                    columns={fieldColumns}
                    rows={schema.fields}
                    rowKey={(f, i) => `${f.path}.${f.tag}.${f.qualifier ?? ""}.${i}`}
                    empty="No fields modeled for this schema."
                  />
                </section>
              </>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
