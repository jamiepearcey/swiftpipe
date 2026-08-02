import type { LucideIcon } from "lucide-react";
import { cn } from "@/lib/utils";
import type { PlaneId } from "@/lib/planes";

const ACCENT: Record<PlaneId, string> = {
  pipeline: "text-plane-pipeline",
  parser: "text-plane-parser",
  books: "text-plane-books",
  control: "text-plane-control",
  regulatory: "text-plane-regulatory",
  system: "text-plane-system",
};

/** A compact KPI tile — label, big tabular value, optional sub-line + icon. */
export function StatTile({
  label,
  value,
  sub,
  icon: Icon,
  accent = "pipeline",
  onClick,
  className,
}: {
  label: string;
  value: React.ReactNode;
  sub?: React.ReactNode;
  icon?: LucideIcon;
  accent?: PlaneId;
  onClick?: () => void;
  className?: string;
}) {
  const Comp = onClick ? "button" : "div";
  return (
    <Comp
      {...(onClick ? { type: "button" as const, onClick } : {})}
      className={cn(
        "flex flex-col rounded-lg border border-outline-subtle bg-surface-panel px-3.5 py-3 text-left",
        onClick && "transition-colors hover:border-outline-strong hover:bg-surface-hover",
        className,
      )}
    >
      <div className="flex items-center gap-1.5">
        {Icon && <Icon className={cn("h-3.5 w-3.5", ACCENT[accent])} />}
        <span className="text-[10.5px] font-medium uppercase tracking-wider text-muted-foreground">{label}</span>
      </div>
      <div className="mt-1.5 text-[22px] font-semibold leading-none tracking-tight tabular-nums text-foreground">
        {value}
      </div>
      {sub != null && <div className="mt-1.5 text-[11.5px] leading-snug text-muted-foreground">{sub}</div>}
    </Comp>
  );
}
