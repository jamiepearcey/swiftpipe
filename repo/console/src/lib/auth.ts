// A tiny auth-state bus. swift-api's HTTP surface can be secured with a bearer
// token; when a fetch comes back 401/403 the console switches from silent
// failure to an explicit "authenticate / token expired" state. The fetch layer
// (api.ts) reports here; views subscribe to surface a banner.
import { useSyncExternalStore } from "react";
import { apiToken } from "@/lib/settings";

export interface AuthState {
  /** Last response status indicating an auth problem (401/403), or 0. */
  status: 0 | 401 | 403;
  hasToken: boolean;
  /** When the failure was observed (ms). */
  at: number;
}

let current: AuthState = { status: 0, hasToken: false, at: 0 };
const listeners = new Set<() => void>();

function emit() {
  listeners.forEach((l) => l());
}

/** Called by the fetch layer when swift-api returns 401/403. */
export function reportAuthFailure(status: 401 | 403): void {
  current = { status, hasToken: apiToken().length > 0, at: Date.now() };
  emit();
}

/** Called after any successful response — clears a prior auth failure. */
export function clearAuthFailure(): void {
  if (current.status === 0) return;
  current = { status: 0, hasToken: apiToken().length > 0, at: Date.now() };
  emit();
}

export const getAuthState = (): AuthState => current;

export function useAuthState(): AuthState {
  return useSyncExternalStore(
    (cb) => {
      listeners.add(cb);
      return () => listeners.delete(cb);
    },
    () => current,
  );
}

export function authMessage(a: AuthState): string {
  if (a.status === 0) return "";
  if (!a.hasToken) {
    return "swift-api requires authentication. Add a bearer token in Settings → Connection.";
  }
  return a.status === 401
    ? "swift-api rejected the bearer token (401). It may be missing, wrong, or expired — update it in Settings → Connection."
    : "The bearer token is not authorized for this resource (403). Check the token in Settings → Connection.";
}
