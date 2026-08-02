import { useEffect, useState } from "react";
import { cn } from "@/lib/utils";
import { health, reconHealth } from "@/lib/api";
import { useAuthState } from "@/lib/auth";
import { VIEW_PLANE, type PlaneId } from "@/lib/planes";
import type { AppRoute, ViewId } from "@/lib/route";
import { viewRoute } from "@/lib/route";
import {
  Boxes,
  FileCode2,
  Gauge,
  Gavel,
  ListChecks,
  Lock,
  Scale,
  Settings as SettingsIcon,
  ScrollText,
  Wallet,
  Workflow,
} from "lucide-react";

type NavItem = { id: ViewId; label: string; icon: typeof Gauge; hint?: string };
type NavSection = { label: string; items: NavItem[] };

const SECTIONS: NavSection[] = [
  {
    label: "Pipeline",
    items: [
      { id: "overview", label: "Overview", icon: Gauge, hint: "Health, throughput, recon at a glance" },
      { id: "parser", label: "Parser", icon: Workflow, hint: "Dissect a SWIFT message" },
      { id: "jobs", label: "Jobs", icon: ListChecks, hint: "Ingest jobs, manifests, artifacts" },
    ],
  },
  {
    label: "Books",
    items: [
      { id: "recon", label: "Reconciliation", icon: Scale, hint: "Statements, cash P&L, breaks" },
      { id: "positions", label: "Positions", icon: Wallet, hint: "Holdings by account" },
    ],
  },
  {
    label: "Regulatory",
    items: [
      { id: "csdr", label: "CSDR penalties", icon: Gavel, hint: "Settlement fail penalties, monthly recon" },
    ],
  },
  {
    label: "Control",
    items: [
      { id: "sources", label: "Sources", icon: Boxes, hint: "Provenance + message types seen" },
      { id: "schemas", label: "Schemas", icon: FileCode2, hint: "The MT schema catalog" },
    ],
  },
  {
    label: "System",
    items: [{ id: "activity", label: "Activity", icon: ScrollText, hint: "This session's operations" }],
  },
];

const ACTIVE_ICON: Record<PlaneId, string> = {
  pipeline: "text-plane-pipeline",
  parser: "text-plane-parser",
  books: "text-plane-books",
  control: "text-plane-control",
  regulatory: "text-plane-regulatory",
  system: "text-plane-system",
};

function SectionLabel({ children }: { children: string }) {
  return (
    <div className="px-2.5 pb-1 pt-0.5 text-[10px] font-semibold uppercase tracking-[0.1em] text-muted-foreground">
      {children}
    </div>
  );
}

function NavButton({
  item,
  active,
  collapsed,
  onSelect,
}: {
  item: NavItem;
  active: boolean;
  collapsed: boolean;
  onSelect: (v: ViewId) => void;
}) {
  const Icon = item.icon;
  const plane = VIEW_PLANE[item.id];
  return (
    <button
      type="button"
      onClick={() => onSelect(item.id)}
      title={item.hint ?? item.label}
      aria-current={active ? "page" : undefined}
      className={cn(
        "flex w-full items-center gap-2.5 rounded-md px-2.5 py-1.5 text-left transition-colors",
        collapsed && "justify-center px-2",
        active ? "bg-surface-active" : "hover:bg-surface-hover",
      )}
    >
      <Icon className={cn("h-4 w-4 shrink-0", active ? ACTIVE_ICON[plane] : "text-icon-muted")} />
      {!collapsed && (
        <span className={cn("block truncate text-[12.5px]", active ? "font-medium text-foreground" : "text-foreground/80")}>
          {item.label}
        </span>
      )}
    </button>
  );
}

function Dot({ up, label }: { up: boolean | null; label: string }) {
  return (
    <span
      title={`${label}: ${up === null ? "checking" : up ? "up" : "down"}`}
      className={cn("h-2 w-2 rounded-full", up === null ? "bg-muted-foreground" : up ? "bg-ok" : "bg-destructive")}
    />
  );
}

const COLLAPSE_KEY = "swiftpipe.sidebar.collapsed";

export function Sidebar({ route, onRoute }: { route: AppRoute; onRoute: (r: AppRoute) => void }) {
  const [apiUp, setApiUp] = useState<boolean | null>(null);
  const [reconUp, setReconUp] = useState<boolean | null>(null);
  const [collapsed, setCollapsed] = useState(() => {
    try {
      return localStorage.getItem(COLLAPSE_KEY) === "1";
    } catch {
      return false;
    }
  });
  const auth = useAuthState();

  useEffect(() => {
    let live = true;
    const tick = () => {
      health().then((h) => live && setApiUp(h));
      reconHealth().then((h) => live && setReconUp(h));
    };
    tick();
    const t = setInterval(tick, 4000);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, []);

  const setCollapsedPersist = (v: boolean) => {
    setCollapsed(v);
    try {
      localStorage.setItem(COLLAPSE_KEY, v ? "1" : "0");
    } catch {
      /* private mode */
    }
  };

  const activeView = route.view;

  return (
    <aside
      className={cn(
        "flex h-full shrink-0 flex-col border-r border-outline-subtle bg-surface-sidebar transition-[width]",
        collapsed ? "w-[56px]" : "w-[236px]",
      )}
    >
      <div className="flex items-center gap-2 border-b border-outline-subtle px-3 py-3">
        <div className="flex h-6 w-6 shrink-0 items-center justify-center rounded-md bg-icon-tile text-icon-tile-foreground">
          <Workflow className="h-3.5 w-3.5" />
        </div>
        {!collapsed && (
          <div className="min-w-0 flex-1 leading-tight">
            <div className="text-[13px] font-semibold tracking-tight">swiftpipe</div>
            <div className="text-[10px] text-muted-foreground">SWIFT ingestion console</div>
          </div>
        )}
        <div className="flex shrink-0 items-center gap-1">
          <Dot up={apiUp} label="swift-api" />
          <Dot up={reconUp} label="recon read-model" />
        </div>
        {apiUp && auth.status !== 0 && !collapsed && (
          <button
            type="button"
            onClick={() => onRoute({ view: "settings", tab: "connection" })}
            title={auth.status === 401 ? "Not authenticated (401)" : "Not authorized (403)"}
            className="text-warn"
          >
            <Lock className="h-3 w-3" />
          </button>
        )}
      </div>

      <nav className="flex flex-1 flex-col gap-3 overflow-y-auto p-2">
        {SECTIONS.map((section, i) => (
          <div key={section.label} className={cn("flex flex-col gap-0.5", i > 0 && "border-t border-outline-subtle pt-3")}>
            {!collapsed && <SectionLabel>{section.label}</SectionLabel>}
            {section.items.map((item) => (
              <NavButton
                key={item.id}
                item={item}
                active={activeView === item.id}
                collapsed={collapsed}
                onSelect={(id) => onRoute(viewRoute(id))}
              />
            ))}
          </div>
        ))}
      </nav>

      <button
        type="button"
        onClick={() => setCollapsedPersist(!collapsed)}
        title={collapsed ? "Expand sidebar" : "Collapse sidebar"}
        className="border-t border-outline-subtle px-3 py-1.5 text-[10.5px] text-muted-foreground hover:bg-surface-hover hover:text-foreground"
      >
        {collapsed ? "»" : "Collapse"}
      </button>

      <button
        type="button"
        aria-current={activeView === "settings" ? "page" : undefined}
        title="Settings"
        onClick={() => onRoute({ view: "settings", tab: "connection" })}
        className={cn(
          "flex items-center gap-2.5 border-t border-outline-subtle px-3 py-2.5 text-left transition-colors",
          collapsed && "justify-center",
          activeView === "settings" ? "bg-surface-active" : "hover:bg-surface-hover",
        )}
      >
        <SettingsIcon className={cn("h-4 w-4", activeView === "settings" ? "text-icon-active" : "text-icon-muted")} />
        {!collapsed && (
          <span className={cn("text-[12.5px]", activeView === "settings" ? "font-medium text-foreground" : "text-foreground/80")}>
            Settings
          </span>
        )}
      </button>
    </aside>
  );
}
