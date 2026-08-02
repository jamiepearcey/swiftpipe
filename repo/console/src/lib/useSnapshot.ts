// Shared recon-snapshot store. Recon, Positions, and Overview all read the same
// ReconSnapshot; this owns a single 10s poll (the recon read-model runs DuckDB
// per request, so be polite) and fans the result out to every subscriber via
// useSyncExternalStore. Degraded mode (stale=true) flows straight through.
import { useSyncExternalStore } from "react";
import { getReconSnapshot, type ReconResult } from "@/lib/api";

export interface SnapshotState {
  result: ReconResult | null;
  error: string | null;
  loading: boolean;
  /** ms of the last successful load. */
  at: number;
}

let state: SnapshotState = { result: null, error: null, loading: true, at: 0 };
const listeners = new Set<() => void>();
let timer: ReturnType<typeof setInterval> | null = null;

function emit() {
  state = { ...state };
  listeners.forEach((l) => l());
}

export async function refreshSnapshot(): Promise<void> {
  try {
    const result = await getReconSnapshot();
    state = { result, error: null, loading: false, at: Date.now() };
  } catch (e) {
    state = { ...state, error: e instanceof Error ? e.message : String(e), loading: false };
  }
  emit();
}

function ensurePolling() {
  if (timer) return;
  refreshSnapshot();
  timer = setInterval(refreshSnapshot, 10_000);
}

function stopPolling() {
  if (listeners.size === 0 && timer) {
    clearInterval(timer);
    timer = null;
  }
}

export function useSnapshot(): SnapshotState & { refresh: () => void } {
  const snap = useSyncExternalStore(
    (cb) => {
      listeners.add(cb);
      ensurePolling();
      return () => {
        listeners.delete(cb);
        stopPolling();
      };
    },
    () => state,
  );
  return { ...snap, refresh: refreshSnapshot };
}
