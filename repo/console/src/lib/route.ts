// Hash router for the swiftpipe console. Flat view set (no multi-tenant tree) —
// a discriminated union parsed from / serialized to the location hash.

export type ViewId =
  | "overview"
  | "parser"
  | "jobs"
  | "recon"
  | "positions"
  | "csdr"
  | "sources"
  | "schemas"
  | "activity"
  | "settings";

export type SettingsTab = "connection" | "appearance";

export type AppRoute =
  | { view: "overview" }
  | { view: "parser"; jobId?: string; msgIndex?: number }
  | { view: "jobs"; jobId?: string }
  | { view: "recon"; messageId?: string }
  | { view: "positions"; account?: string }
  | { view: "csdr"; txnRef?: string }
  | { view: "sources" }
  | { view: "schemas"; mt?: string }
  | { view: "activity" }
  | { view: "settings"; tab: SettingsTab };

const VIEWS: ViewId[] = [
  "overview",
  "parser",
  "jobs",
  "recon",
  "positions",
  "csdr",
  "sources",
  "schemas",
  "activity",
  "settings",
];

export function parseHash(hash = location.hash): AppRoute {
  const raw = hash.replace(/^#\/?/, "").replace(/\/+$/, "");
  if (!raw) return { view: "overview" };
  const parts = raw.split("/").map(decodeURIComponent);
  const head = parts[0] as ViewId;
  if (!VIEWS.includes(head)) return { view: "overview" };

  switch (head) {
    case "parser": {
      const jobId = parts[1];
      const msgIndex = parts[2] != null ? Number(parts[2]) : undefined;
      return { view: "parser", jobId, msgIndex: Number.isFinite(msgIndex) ? msgIndex : undefined };
    }
    case "jobs":
      return { view: "jobs", jobId: parts[1] };
    case "recon":
      return { view: "recon", messageId: parts[1] };
    case "positions":
      return { view: "positions", account: parts[1] };
    case "csdr":
      return { view: "csdr", txnRef: parts[1] };
    case "schemas":
      return { view: "schemas", mt: parts[1]?.toUpperCase() };
    case "settings": {
      const tab: SettingsTab = parts[1] === "appearance" ? "appearance" : "connection";
      return { view: "settings", tab };
    }
    default:
      return { view: head } as AppRoute;
  }
}

export function routeToHash(route: AppRoute): string {
  switch (route.view) {
    case "parser":
      return route.jobId
        ? `/parser/${encodeURIComponent(route.jobId)}${route.msgIndex != null ? `/${route.msgIndex}` : ""}`
        : "/parser";
    case "jobs":
      return route.jobId ? `/jobs/${encodeURIComponent(route.jobId)}` : "/jobs";
    case "recon":
      return route.messageId ? `/recon/${encodeURIComponent(route.messageId)}` : "/recon";
    case "positions":
      return route.account ? `/positions/${encodeURIComponent(route.account)}` : "/positions";
    case "csdr":
      return route.txnRef ? `/csdr/${encodeURIComponent(route.txnRef)}` : "/csdr";
    case "schemas":
      return route.mt ? `/schemas/${encodeURIComponent(route.mt)}` : "/schemas";
    case "settings":
      return route.tab === "appearance" ? "/settings/appearance" : "/settings";
    default:
      return `/${route.view}`;
  }
}

export function routesEqual(a: AppRoute, b: AppRoute): boolean {
  return routeToHash(a) === routeToHash(b);
}

/** Navigate to a view by id with default params. */
export function viewRoute(view: ViewId): AppRoute {
  if (view === "settings") return { view: "settings", tab: "connection" };
  return { view } as AppRoute;
}
