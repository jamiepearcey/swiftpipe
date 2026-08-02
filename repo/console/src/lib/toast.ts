// A tiny toast bus. Ephemeral, dismissable notifications for the results of
// user actions — decoupled from any view so anything can raise one.
export type ToastTone = "ok" | "info" | "warn" | "error";
export interface Toast {
  id: number;
  tone: ToastTone;
  title: string;
  detail?: string;
  /** ms before auto-dismiss; 0 = sticky until dismissed. */
  ttl: number;
}

let seq = 0;
const live: Toast[] = [];
const subs = new Set<(t: Toast[]) => void>();

function emit() {
  const snapshot = [...live];
  subs.forEach((cb) => cb(snapshot));
}

export function toast(tone: ToastTone, title: string, detail?: string, ttl = 3600): number {
  const t: Toast = { id: seq++, tone, title, detail, ttl };
  live.unshift(t);
  if (live.length > 6) live.length = 6;
  emit();
  return t.id;
}

export function dismissToast(id: number) {
  const i = live.findIndex((t) => t.id === id);
  if (i >= 0) {
    live.splice(i, 1);
    emit();
  }
}

export const getToasts = () => live;
export function subscribeToasts(cb: (t: Toast[]) => void): () => void {
  subs.add(cb);
  return () => subs.delete(cb);
}
