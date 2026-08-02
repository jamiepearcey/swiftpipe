import { useMemo, useState } from "react";
import { cn } from "@/lib/utils";
import { ChevronDown, ChevronUp } from "lucide-react";

export interface Column<T> {
  key: string;
  header: string;
  align?: "left" | "right";
  mono?: boolean;
  /** Cell content. */
  render: (row: T) => React.ReactNode;
  /** Sort key; return a number or string. Omit to disable sorting for the col. */
  sortValue?: (row: T) => number | string;
  className?: string;
}

/**
 * Compact, sortable, sticky-header table. Deliberately plain — at swiftpipe row
 * counts a grid library is unwarranted (Fable spec §6: do not spend on grids).
 */
export function DataTable<T>({
  columns,
  rows,
  rowKey,
  onRowClick,
  activeRow,
  flashRow,
  empty = "No rows.",
  dense,
  className,
}: {
  columns: Column<T>[];
  rows: T[];
  rowKey: (row: T, i: number) => string;
  onRowClick?: (row: T) => void;
  activeRow?: (row: T) => boolean;
  flashRow?: (row: T) => boolean;
  empty?: React.ReactNode;
  dense?: boolean;
  className?: string;
}) {
  const [sort, setSort] = useState<{ key: string; dir: 1 | -1 } | null>(null);

  const sorted = useMemo(() => {
    if (!sort) return rows;
    const col = columns.find((c) => c.key === sort.key);
    if (!col?.sortValue) return rows;
    const sv = col.sortValue;
    return [...rows].sort((a, b) => {
      const va = sv(a);
      const vb = sv(b);
      if (va < vb) return -1 * sort.dir;
      if (va > vb) return 1 * sort.dir;
      return 0;
    });
  }, [rows, sort, columns]);

  const toggleSort = (c: Column<T>) => {
    if (!c.sortValue) return;
    setSort((s) => (s?.key === c.key ? { key: c.key, dir: s.dir === 1 ? -1 : 1 } : { key: c.key, dir: 1 }));
  };

  if (!rows.length) {
    return <div className="rounded-lg border border-dashed border-outline-subtle p-8 text-center text-[12.5px] text-muted-foreground">{empty}</div>;
  }

  return (
    <div className={cn("overflow-auto rounded-lg border border-outline-subtle", className)}>
      <table className="w-full border-collapse text-[12px]">
        <thead className="sticky top-0 z-10 bg-surface-header">
          <tr className="border-b border-outline-subtle">
            {columns.map((c) => (
              <th
                key={c.key}
                onClick={() => toggleSort(c)}
                className={cn(
                  "select-none px-3 py-2 font-medium text-muted-foreground",
                  c.align === "right" ? "text-right" : "text-left",
                  c.sortValue && "cursor-pointer hover:text-foreground",
                )}
              >
                <span className={cn("inline-flex items-center gap-1", c.align === "right" && "flex-row-reverse")}>
                  {c.header}
                  {sort?.key === c.key &&
                    (sort.dir === 1 ? <ChevronUp className="h-3 w-3" /> : <ChevronDown className="h-3 w-3" />)}
                </span>
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {sorted.map((row, i) => (
            <tr
              key={rowKey(row, i)}
              onClick={onRowClick ? () => onRowClick(row) : undefined}
              className={cn(
                "border-b border-outline-subtle/60 transition-colors",
                onRowClick && "cursor-pointer",
                activeRow?.(row) ? "bg-primary/10" : "hover:bg-surface-hover",
                flashRow?.(row) && "animate-flash-row",
              )}
            >
              {columns.map((c) => (
                <td
                  key={c.key}
                  className={cn(
                    dense ? "px-3 py-1" : "px-3 py-1.5",
                    c.align === "right" ? "text-right" : "text-left",
                    c.mono && "font-mono tabular-nums",
                    "text-foreground/90",
                    c.className,
                  )}
                >
                  {c.render(row)}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
