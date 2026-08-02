// Small formatting helpers. Amounts are always rendered tabular + right-aligned
// by the callers; these just produce the strings.

const AMOUNT = new Intl.NumberFormat("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 });

/** Fixed 2dp with thousands separators. */
export function fmtAmount(n: number): string {
  return AMOUNT.format(n);
}

/** Signed amount with an explicit leading + / − (− shown for negatives). */
export function fmtSigned(n: number): string {
  const s = fmtAmount(Math.abs(n));
  return n < 0 ? `−${s}` : `+${s}`;
}

export function fmtInt(n: number): string {
  return new Intl.NumberFormat("en-US").format(n);
}

/** SWIFT YYMMDD → ISO YYYY-MM-DD (20xx pivot). Passes through anything else. */
export function swiftDate(raw?: string): string {
  if (!raw) return "";
  const m = raw.match(/^(\d{2})(\d{2})(\d{2})$/);
  if (!m) return raw;
  return `20${m[1]}-${m[2]}-${m[3]}`;
}

export function fmtMs(ms?: number): string {
  if (ms == null) return "—";
  if (ms < 1000) return `${Math.round(ms)} ms`;
  return `${(ms / 1000).toFixed(2)} s`;
}

/** Relative time from an epoch-ms or ISO string. */
export function relTime(at: number | string): string {
  const t = typeof at === "number" ? at : Date.parse(at);
  if (!Number.isFinite(t)) return "";
  const d = Math.max(0, Date.now() - t);
  const s = Math.round(d / 1000);
  if (s < 5) return "just now";
  if (s < 60) return `${s}s ago`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h}h ago`;
  return `${Math.round(h / 24)}d ago`;
}
