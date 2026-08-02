// A small in-app activity bus. Console operations (parses, uploads, job
// transitions, snapshot refreshes) log here so the Activity view is a single
// timeline of everything the operator did in this session. Local-only — no
// backend event stream exists yet.
export interface ActivityEntry {
  id: number;
  kind: string;
  detail: string;
  tone: "ok" | "info" | "warn" | "error";
  at: number;
}

let seq = 0;
const buf: ActivityEntry[] = [];
const subs = new Set<(e: ActivityEntry) => void>();

export function logActivity(
  kind: string,
  detail: string,
  tone: ActivityEntry["tone"] = "info",
): void {
  const e: ActivityEntry = { id: seq++, kind, detail, tone, at: Date.now() };
  buf.unshift(e);
  if (buf.length > 300) buf.length = 300;
  subs.forEach((cb) => cb(e));
}

export const getActivity = () => buf;

export function clearActivity() {
  buf.length = 0;
  subs.forEach((cb) => cb({ id: seq++, kind: "__cleared__", detail: "", tone: "info", at: Date.now() }));
}

export function subscribeActivity(cb: (e: ActivityEntry) => void): () => void {
  subs.add(cb);
  return () => subs.delete(cb);
}
