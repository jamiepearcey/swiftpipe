import { useEffect, useRef, useState } from "react";
import { cn } from "@/lib/utils";
import { dismissToast, getToasts, subscribeToasts, type Toast } from "@/lib/toast";
import { CheckCircle2, Info, AlertTriangle, XCircle, X } from "lucide-react";

const ICON = { ok: CheckCircle2, info: Info, warn: AlertTriangle, error: XCircle } as const;
const ICON_TONE = { ok: "text-ok", info: "text-info", warn: "text-warn", error: "text-destructive" } as const;

export function Toaster() {
  const [toasts, setToasts] = useState<Toast[]>(() => [...getToasts()]);
  const timers = useRef(new Map<number, ReturnType<typeof setTimeout>>());

  useEffect(() => subscribeToasts(setToasts), []);

  useEffect(() => {
    for (const t of toasts) {
      if (t.ttl > 0 && !timers.current.has(t.id)) {
        timers.current.set(
          t.id,
          setTimeout(() => {
            dismissToast(t.id);
            timers.current.delete(t.id);
          }, t.ttl),
        );
      }
    }
  }, [toasts]);

  if (!toasts.length) return null;
  return (
    <div className="pointer-events-none fixed bottom-4 right-4 z-[60] flex w-[320px] flex-col gap-2">
      {toasts.map((t) => {
        const Icon = ICON[t.tone];
        return (
          <div
            key={t.id}
            role="status"
            className={cn(
              "pointer-events-auto flex items-start gap-2.5 rounded-lg border border-outline-strong bg-surface-panel px-3 py-2.5 shadow-lg shadow-black/40 backdrop-blur-sm",
              "animate-in fade-in slide-in-from-bottom-2 duration-200",
            )}
          >
            <Icon className={cn("mt-0.5 h-4 w-4 shrink-0", ICON_TONE[t.tone])} />
            <div className="min-w-0 flex-1">
              <div className="text-[12.5px] font-medium text-foreground">{t.title}</div>
              {t.detail && <div className="truncate font-mono text-[11px] text-muted-foreground">{t.detail}</div>}
            </div>
            <button
              onClick={() => dismissToast(t.id)}
              aria-label="Dismiss"
              className="shrink-0 rounded p-0.5 text-muted-foreground hover:bg-surface-hover hover:text-foreground"
            >
              <X className="h-3.5 w-3.5" />
            </button>
          </div>
        );
      })}
    </div>
  );
}
