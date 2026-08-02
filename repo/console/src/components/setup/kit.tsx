// Shared building blocks — cards, buttons, fields, segmented controls.
import { cn } from "@/lib/utils";

export const inputCls =
  "w-full rounded-md border border-outline-strong bg-surface-app px-2.5 py-1.5 text-[12px] outline-none placeholder:text-muted-foreground/60 focus:border-icon-active";

export function errText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export function Card({
  title,
  desc,
  right,
  children,
  className,
}: {
  title?: string;
  desc?: string;
  right?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <section className={cn("rounded-lg border border-outline-subtle bg-surface-sidebar", className)}>
      {(title || right) && (
        <div className="flex items-center gap-2 border-b border-outline-subtle px-4 py-2.5">
          <div>
            {title && <div className="text-[12.5px] font-semibold text-foreground">{title}</div>}
            {desc && <div className="text-[11px] text-muted-foreground">{desc}</div>}
          </div>
          {right && <div className="ml-auto">{right}</div>}
        </div>
      )}
      <div className="p-4">{children}</div>
    </section>
  );
}

export function Btn({
  children,
  onClick,
  disabled,
  variant = "default",
  className,
  type = "button",
}: {
  children: React.ReactNode;
  onClick?: () => void;
  disabled?: boolean;
  variant?: "default" | "primary" | "danger" | "ghost";
  className?: string;
  type?: "button" | "submit";
}) {
  return (
    <button
      type={type}
      onClick={onClick}
      disabled={disabled}
      className={cn(
        "rounded-md px-3 py-1.5 text-[12px] font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-50",
        variant === "primary"
          ? "bg-icon-tile text-icon-tile-foreground hover:opacity-90"
          : variant === "danger"
            ? "border border-destructive/45 text-destructive hover:bg-destructive/10"
            : variant === "ghost"
              ? "text-muted-foreground hover:bg-surface-hover"
              : "border border-outline-strong text-foreground/85 hover:bg-surface-hover",
        className,
      )}
    >
      {children}
    </button>
  );
}

export function Segmented<T extends string>({
  options,
  value,
  onChange,
}: {
  options: readonly { id: T; label: string; title?: string }[];
  value: T;
  onChange: (v: T) => void;
}) {
  return (
    <div className="flex overflow-hidden rounded-md border border-outline-strong text-[11px]">
      {options.map((o) => (
        <button
          key={o.id}
          type="button"
          title={o.title}
          onClick={() => onChange(o.id)}
          className={cn(
            "px-2.5 py-1 transition-colors",
            value === o.id ? "bg-surface-active text-foreground" : "text-muted-foreground hover:bg-surface-hover",
          )}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="block">
      <span className="mb-1 block text-[11px] text-muted-foreground">{label}</span>
      {children}
    </label>
  );
}
