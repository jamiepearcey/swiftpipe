// Plain HTML table for arbitrary row objects (job manifest message lists, raw
// object previews). For domain tables prefer DataTable.
import { useMemo } from "react";

function fmt(v: unknown): string {
  if (v === null || v === undefined) return "∅";
  if (typeof v === "number") return Number.isInteger(v) ? String(v) : v.toFixed(6).replace(/\.?0+$/, "");
  if (typeof v === "object") return JSON.stringify(v);
  return String(v);
}

function collectColumns(rows: Record<string, unknown>[]) {
  const seen = new Set<string>();
  for (const row of rows) for (const key of Object.keys(row)) seen.add(key);
  return [...seen];
}

export function OutputTable({ rows }: { rows: Record<string, unknown>[] }) {
  const cols = useMemo(() => collectColumns(rows), [rows]);
  if (!rows.length) return <div className="p-6 text-sm text-muted-foreground">No rows.</div>;
  return (
    <div className="max-h-full overflow-auto rounded-md border border-outline-subtle">
      <table className="w-full border-collapse text-[12px]">
        <thead className="sticky top-0 z-10 bg-surface-header">
          <tr className="border-b border-outline-subtle">
            {cols.map((c) => (
              <th key={c} className="px-3 py-2 text-left font-mono font-medium text-muted-foreground">
                {c}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((r, i) => (
            <tr key={i} className="border-b border-outline-subtle/70 hover:bg-surface-hover">
              {cols.map((c) => (
                <td key={c} className="px-3 py-1.5 font-mono tabular-nums text-foreground/90">
                  {fmt(r[c])}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
