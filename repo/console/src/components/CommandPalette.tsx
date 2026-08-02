import { useEffect, useMemo, useState } from "react";
import { cn } from "@/lib/utils";
import type { AppRoute, ViewId } from "@/lib/route";
import { viewRoute } from "@/lib/route";
import { VIEW_PLANE, type PlaneId } from "@/lib/planes";
import { getSettings, setTheme } from "@/lib/settings";
import {
  Boxes,
  FileCode2,
  Gauge,
  ListChecks,
  Moon,
  Scale,
  Search,
  Settings as SettingsIcon,
  ScrollText,
  Sun,
  Wallet,
  Workflow,
} from "lucide-react";

type IconType = typeof Gauge;

interface Command {
  id: string;
  label: string;
  hint?: string;
  icon: IconType;
  iconClassName: string;
  run: () => void;
}

const PLANE_ICON_CLASS: Record<PlaneId, string> = {
  pipeline: "text-plane-pipeline",
  parser: "text-plane-parser",
  books: "text-plane-books",
  control: "text-plane-control",
  regulatory: "text-plane-regulatory",
  system: "text-plane-system",
};

const NAV_ITEMS: { view: ViewId; label: string; hint: string; icon: IconType }[] = [
  { view: "overview", label: "Overview", hint: "Health, throughput, recon at a glance", icon: Gauge },
  { view: "parser", label: "Parser", hint: "Dissect a SWIFT message", icon: Workflow },
  { view: "jobs", label: "Jobs", hint: "Ingest jobs, manifests, artifacts", icon: ListChecks },
  { view: "recon", label: "Reconciliation", hint: "Statements, cash P&L, breaks", icon: Scale },
  { view: "positions", label: "Positions", hint: "Holdings by account", icon: Wallet },
  { view: "sources", label: "Sources", hint: "Provenance + message types seen", icon: Boxes },
  { view: "schemas", label: "Schemas", hint: "The MT schema catalog", icon: FileCode2 },
  { view: "activity", label: "Activity", hint: "This session's operations", icon: ScrollText },
  { view: "settings", label: "Settings", hint: "Connection + appearance", icon: SettingsIcon },
];

export function CommandPalette({ onRoute }: { onRoute: (r: AppRoute) => void }) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setOpen((v) => !v);
        return;
      }
      if (e.key === "Escape" && open) {
        setOpen(false);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open]);

  useEffect(() => {
    if (open) {
      setQuery("");
      setIndex(0);
    }
  }, [open]);

  const commands: Command[] = useMemo(() => {
    const theme = getSettings().theme;
    const navCommands: Command[] = NAV_ITEMS.map((item) => ({
      id: `nav:${item.view}`,
      label: item.label,
      hint: item.hint,
      icon: item.icon,
      iconClassName: PLANE_ICON_CLASS[VIEW_PLANE[item.view]],
      run: () => onRoute(viewRoute(item.view)),
    }));
    const actionCommands: Command[] = [
      {
        id: "action:toggle-theme",
        label: "Toggle theme (light/dark)",
        hint: theme === "dark" ? "Currently dark" : "Currently light",
        icon: theme === "dark" ? Sun : Moon,
        iconClassName: "text-plane-system",
        run: () => setTheme(theme === "dark" ? "light" : "dark"),
      },
      {
        id: "action:open-parser",
        label: "Open SWIFT parser",
        icon: Workflow,
        iconClassName: PLANE_ICON_CLASS[VIEW_PLANE.parser],
        run: () => onRoute(viewRoute("parser")),
      },
    ];
    return [...navCommands, ...actionCommands];
  }, [onRoute]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return commands;
    return commands.filter((c) => c.label.toLowerCase().includes(q));
  }, [commands, query]);

  const clampedIndex = filtered.length ? Math.min(index, filtered.length - 1) : 0;

  if (!open) return null;

  const close = () => setOpen(false);

  const runAt = (i: number) => {
    const cmd = filtered[i];
    if (!cmd) return;
    cmd.run();
    close();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setIndex((i) => (filtered.length ? (i + 1) % filtered.length : 0));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setIndex((i) => (filtered.length ? (i - 1 + filtered.length) % filtered.length : 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      runAt(clampedIndex);
    } else if (e.key === "Escape") {
      e.preventDefault();
      close();
    }
  };

  return (
    <div
      className="fixed inset-0 z-[70] bg-black/50 backdrop-blur-sm"
      onClick={close}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Command palette"
        onClick={(e) => e.stopPropagation()}
        className="mx-auto mt-[15vh] max-w-lg overflow-hidden rounded-xl border border-outline-strong bg-surface-panel shadow-xl"
      >
        <div className="flex items-center gap-2 border-b border-outline-subtle px-3 py-2.5">
          <Search className="h-4 w-4 shrink-0 text-icon-muted" />
          <input
            autoFocus
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setIndex(0);
            }}
            onKeyDown={onKeyDown}
            placeholder="Jump to… (type a view or action)"
            className="w-full border-none bg-transparent text-[14px] text-foreground outline-none placeholder:text-muted-foreground"
          />
        </div>

        <div className="max-h-[50vh] overflow-y-auto p-1.5">
          {filtered.length === 0 && (
            <div className="px-3 py-4 text-center text-[12.5px] text-muted-foreground">No matches</div>
          )}
          {filtered.map((cmd, i) => {
            const Icon = cmd.icon;
            const active = i === clampedIndex;
            return (
              <button
                key={cmd.id}
                type="button"
                onMouseEnter={() => setIndex(i)}
                onClick={() => runAt(i)}
                className={cn(
                  "flex w-full items-center gap-2.5 rounded-md px-2.5 py-1.5 text-left transition-colors",
                  active ? "bg-surface-active" : "hover:bg-surface-hover",
                )}
              >
                <Icon className={cn("h-4 w-4 shrink-0", cmd.iconClassName)} />
                <span className="flex-1 truncate text-[13px] text-foreground">{cmd.label}</span>
                {cmd.hint && (
                  <span className="shrink-0 truncate text-[11px] text-muted-foreground">{cmd.hint}</span>
                )}
              </button>
            );
          })}
        </div>

        <div className="border-t border-outline-subtle px-3 py-1.5 text-[10.5px] text-muted-foreground">
          ↑↓ navigate · ↵ select · esc close · ⌘K toggle
        </div>
      </div>
    </div>
  );
}
