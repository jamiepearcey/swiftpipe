import { cn } from "@/lib/utils";

export type TagTone = "muted" | "active" | "ok" | "warn" | "error";

const TONE: Record<TagTone, string> = {
  muted: "border-outline-strong bg-surface-toolbar text-muted-foreground",
  active: "border-plane-parser/45 bg-plane-parser/12 text-plane-parser",
  ok: "border-ok/40 bg-ok/12 text-ok",
  warn: "border-warn/45 bg-warn/12 text-warn",
  error: "border-destructive/45 bg-destructive/12 text-destructive",
};

/**
 * The console's SWIFT lingua franca — a monospace :NN: tag chip that appears
 * in the parser, the schema browser, and error messages so the whole console
 * visibly "speaks SWIFT".
 */
export function TagChip({
  tag,
  tone = "muted",
  onClick,
  title,
  className,
}: {
  tag: string;
  tone?: TagTone;
  onClick?: () => void;
  title?: string;
  className?: string;
}) {
  const Comp = onClick ? "button" : "span";
  return (
    <Comp
      {...(onClick ? { type: "button" as const, onClick } : {})}
      title={title}
      className={cn(
        "inline-flex items-center rounded border px-1 py-0.5 font-mono text-[10.5px] font-semibold leading-none",
        TONE[tone],
        onClick && "transition-colors hover:brightness-125",
        className,
      )}
    >
      :{tag}:
    </Comp>
  );
}
