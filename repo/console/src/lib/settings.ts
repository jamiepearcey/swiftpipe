// Console settings, persisted to localStorage. Connection config for the two
// swiftpipe surfaces (swift-api jobs + the recon read-model), the bearer token,
// and theme. Defaults point at the Vite dev proxy paths (/api, /recon).
import { useSyncExternalStore } from "react";

export type ThemeMode = "dark" | "light";

export interface Settings {
  connection: {
    /** Base for swift-api (jobs/upload/manifest/metrics). Proxied in dev. */
    apiBase: string;
    /** Base for the `ingest serve` recon read-model. Proxied in dev. */
    reconBase: string;
    /** Base for the CSDR penalty read-model (same `ingest serve`). Proxied in dev. */
    csdrBase: string;
    /** Bearer token for swift-api. Empty = unsecured (no header sent). */
    bearerToken: string;
    /** Static recon-snapshot.json to fall back to when the recon server is down. */
    reconFallback: string;
    /** Static csdr-snapshot.json to fall back to when the CSDR server is down. */
    csdrFallback: string;
  };
  theme: ThemeMode;
}

export const defaultSettings: Settings = {
  connection: {
    apiBase: "/api",
    reconBase: "/recon",
    csdrBase: "/csdr",
    bearerToken: "",
    reconFallback: "/recon-snapshot.json",
    csdrFallback: "/csdr-snapshot.json",
  },
  theme: "dark",
};

const KEY = "swiftpipe-console:settings";

function load(): Settings {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return defaultSettings;
    const parsed = JSON.parse(raw);
    return {
      connection: { ...defaultSettings.connection, ...parsed.connection },
      theme: parsed.theme === "light" ? "light" : "dark",
    };
  } catch {
    return defaultSettings;
  }
}

let current = load();
const listeners = new Set<() => void>();

export const getSettings = (): Settings => current;

export function setSettings(next: Settings): void {
  current = next;
  try {
    localStorage.setItem(KEY, JSON.stringify(next));
  } catch {
    /* ignore quota / private-mode errors */
  }
  listeners.forEach((l) => l());
}

export function patchConnection(patch: Partial<Settings["connection"]>): void {
  setSettings({ ...current, connection: { ...current.connection, ...patch } });
}

export function setTheme(theme: ThemeMode): void {
  setSettings({ ...current, theme });
}

export function applyTheme(theme: ThemeMode = current.theme): void {
  const el = document.documentElement;
  el.classList.toggle("light", theme === "light");
  el.classList.toggle("dark", theme === "dark");
}

if (typeof document !== "undefined") {
  applyTheme();
  listeners.add(() => applyTheme());
}

export function useSettings(): Settings {
  return useSyncExternalStore(
    (cb) => {
      listeners.add(cb);
      return () => listeners.delete(cb);
    },
    () => current,
  );
}

/** Bearer token for swift-api, or "" when none is configured. */
export const apiToken = (): string => current.connection.bearerToken.trim();
