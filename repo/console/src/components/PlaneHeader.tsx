import { cn } from "@/lib/utils";
import { PLANES, type PlaneId } from "@/lib/planes";

const RAIL: Record<PlaneId, string> = {
  pipeline: "border-plane-pipeline",
  parser: "border-plane-parser",
  books: "border-plane-books",
  control: "border-plane-control",
  regulatory: "border-plane-regulatory",
  system: "border-plane-system",
};

const CHIP: Record<PlaneId, string> = {
  pipeline: "bg-plane-pipeline/15 text-plane-pipeline",
  parser: "bg-plane-parser/15 text-plane-parser",
  books: "bg-plane-books/15 text-plane-books",
  control: "bg-plane-control/15 text-plane-control",
  regulatory: "bg-plane-regulatory/15 text-plane-regulatory",
  system: "bg-plane-system/15 text-plane-system",
};

/**
 * Plane-tagged page chrome. Row 1: plane chip + route (+ optional breadcrumb).
 * Row 2: title / summary / actions.
 */
export function PlaneHeader({
  plane,
  title,
  route,
  summary,
  planeLabel,
  breadcrumb,
  aside,
  actions,
  children,
  className,
}: {
  plane: PlaneId;
  title: string;
  route: string;
  summary: string;
  planeLabel?: string;
  breadcrumb?: React.ReactNode;
  aside?: React.ReactNode;
  actions?: React.ReactNode;
  children?: React.ReactNode;
  className?: string;
}) {
  const p = PLANES[plane];
  return (
    <header className={cn("shrink-0 border-b border-outline-subtle bg-surface-panel", className)}>
      <div className={cn("flex gap-0 border-l-[3px]", RAIL[plane])}>
        <div className="min-w-0 flex-1">
          <div className="flex min-w-0 flex-wrap items-center gap-2 border-b border-outline-subtle/70 px-6 py-2">
            <span
              className={cn(
                "shrink-0 rounded px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider",
                CHIP[plane],
              )}
            >
              {planeLabel ?? p.label}
            </span>
            <code className="shrink-0 rounded border border-outline-strong bg-surface-toolbar px-1.5 py-0.5 font-mono text-[10.5px] text-muted-foreground">
              {route}
            </code>
            {breadcrumb ? (
              <>
                <span className="hidden text-outline-strong/60 sm:inline" aria-hidden>
                  ·
                </span>
                <div className="min-w-0 flex-1">{breadcrumb}</div>
              </>
            ) : null}
          </div>
          <div className="px-6 py-3">
            <div className="flex flex-wrap items-start gap-3">
              <div className="min-w-0 flex-1">
                <h1 className="text-[17px] font-semibold tracking-tight">{title}</h1>
                <p className="mt-0.5 max-w-2xl text-[12.5px] leading-snug text-muted-foreground">{summary}</p>
              </div>
              {actions}
            </div>
            {children}
          </div>
        </div>
        {aside ? (
          <aside className="hidden w-[240px] shrink-0 flex-col justify-center border-l border-outline-subtle px-4 py-3 sm:flex">
            {aside}
          </aside>
        ) : null}
      </div>
    </header>
  );
}
