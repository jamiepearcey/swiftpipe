import { Fragment } from "react";
import { cn } from "@/lib/utils";

export type CrumbPart = {
  label: string;
  onClick?: () => void;
  mono?: boolean;
};

/** Compact path trail for the first row of a PlaneHeader. */
export function Crumb({ parts, className }: { parts: CrumbPart[]; className?: string }) {
  return (
    <nav
      aria-label="Breadcrumb"
      className={cn("flex min-w-0 flex-wrap items-center gap-1.5 text-[11.5px]", className)}
    >
      {parts.map((p, i) => (
        <Fragment key={`${p.label}-${i}`}>
          {i > 0 ? <span className="text-outline-strong/80">/</span> : null}
          {p.onClick ? (
            <button
              type="button"
              onClick={p.onClick}
              className={cn(
                "truncate text-muted-foreground transition-colors hover:text-foreground",
                p.mono && "font-mono text-[11px]",
              )}
            >
              {p.label}
            </button>
          ) : (
            <span
              className={cn(
                "truncate font-medium text-foreground/90",
                p.mono && "font-mono text-[11px] font-normal text-muted-foreground",
              )}
            >
              {p.label}
            </span>
          )}
        </Fragment>
      ))}
    </nav>
  );
}
