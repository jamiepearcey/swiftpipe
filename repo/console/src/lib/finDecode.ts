// Best-effort decoders for the common cash/securities FIN tags, so the parser
// workbench can show decoded sub-fields (not just raw text) for block-4 tags.
// Deliberately lenient: unknown shapes fall back to the raw value. These are
// presentation helpers, not the authoritative schema parse (that is swift-core
// / swift-schema on the backend).
import type { FinField } from "@/lib/fin";
import { swiftDate } from "@/lib/format";

/** SWIFT decimal uses a comma: "25000,00" → 25000.00. */
export function swiftAmount(raw: string): number {
  const n = Number(raw.replace(/\./g, "").replace(",", "."));
  return Number.isFinite(n) ? n : NaN;
}

export interface DecodedBalance {
  dc: "C" | "D";
  date: string;
  currency: string;
  amount: number;
  signed: number;
}

/** :60F:/:62F:/:64: → C/D + YYMMDD + CCY + amount. */
export function decodeBalance(value: string): DecodedBalance | null {
  const m = value.match(/^([CD])(\d{6})([A-Z]{3})([\d.,]+)$/);
  if (!m) return null;
  const amount = swiftAmount(m[4]);
  const dc = m[1] as "C" | "D";
  return { dc, date: swiftDate(m[2]), currency: m[3], amount, signed: dc === "C" ? amount : -amount };
}

export interface DecodedStatementLine {
  valueDate: string;
  entryDate?: string;
  dc: "C" | "D" | "RC" | "RD";
  amount: number;
  signed: number;
  fundsCode?: string;
  transactionType?: string;
  customerRef?: string;
  bankRef?: string;
}

/** :61: statement line — valueDate, [entryDate], D/C mark, amount, type, refs. */
export function decodeStatementLine(value: string): DecodedStatementLine | null {
  const m = value.match(/^(\d{6})(\d{4})?(RC|RD|C|D)([\d.,]+)([A-Z])([A-Z0-9]{3})(.*)$/);
  if (!m) return null;
  const dc = m[3] as DecodedStatementLine["dc"];
  const amount = swiftAmount(m[4]);
  const sign = dc === "C" || dc === "RD" ? 1 : -1;
  const [customerRef, bankRef] = m[7].split("//");
  return {
    valueDate: swiftDate(m[1]),
    entryDate: m[2] ? m[2] : undefined,
    dc,
    amount,
    signed: sign * amount,
    fundsCode: m[5],
    transactionType: m[6],
    customerRef: customerRef || undefined,
    bankRef: bankRef || undefined,
  };
}

export interface DecodedChip {
  label: string;
  value: string;
  tone?: "credit" | "debit" | "muted";
}

/** Turn a field into labeled chips for the tree row (best effort). */
export function decodeChips(field: FinField): DecodedChip[] {
  const v = field.value.trim();
  const base = field.tag.replace(/[A-Z]$/, ""); // 60F -> 60
  if (["60", "62", "64", "65"].includes(base) || field.tag.startsWith("60") || field.tag.startsWith("62")) {
    const b = decodeBalance(v);
    if (b)
      return [
        { label: "D/C", value: b.dc === "C" ? "Credit" : "Debit", tone: b.dc === "C" ? "credit" : "debit" },
        { label: "Date", value: b.date },
        { label: "Amount", value: `${b.currency} ${b.amount.toLocaleString("en-US", { minimumFractionDigits: 2 })}` },
      ];
  }
  if (field.tag === "61") {
    const l = decodeStatementLine(v);
    if (l)
      return [
        { label: "Value", value: l.valueDate },
        { label: "D/C", value: l.dc, tone: l.signed >= 0 ? "credit" : "debit" },
        { label: "Amount", value: l.amount.toLocaleString("en-US", { minimumFractionDigits: 2 }), tone: l.signed >= 0 ? "credit" : "debit" },
        ...(l.transactionType ? [{ label: "Type", value: l.transactionType }] : []),
        ...(l.customerRef ? [{ label: "Ref", value: l.customerRef }] : []),
      ];
  }
  if (field.tag === "35B") {
    const isin = field.lines.find((ln) => ln.includes("ISIN"))?.replace(/.*ISIN\s+/, "").trim();
    const desc = field.lines.filter((ln) => !ln.includes("ISIN")).join(" ").trim();
    return [...(isin ? [{ label: "ISIN", value: isin }] : []), ...(desc ? [{ label: "Instrument", value: desc }] : [])];
  }
  return [];
}

export interface CashRecon {
  opening: number;
  closing: number;
  net: number;
  expected: number;
  diff: number;
  reconciles: boolean;
  currency: string;
  entryCount: number;
}

/** Derive an MT940-style reconciliation equation from parsed block-4 fields. */
export function cashRecon(fields: FinField[]): CashRecon | null {
  const open = fields.find((f) => f.tag === "60F" || f.tag === "60M");
  const close = fields.find((f) => f.tag === "62F" || f.tag === "62M");
  if (!open || !close) return null;
  const ob = decodeBalance(open.value.trim());
  const cb = decodeBalance(close.value.trim());
  if (!ob || !cb) return null;
  const lines = fields.filter((f) => f.tag === "61").map((f) => decodeStatementLine(f.value.trim()));
  const net = lines.reduce((a, l) => a + (l?.signed ?? 0), 0);
  const expected = ob.signed + net;
  const diff = cb.signed - expected;
  return {
    opening: ob.signed,
    closing: cb.signed,
    net,
    expected,
    diff,
    reconciles: Math.abs(diff) < 0.005,
    currency: ob.currency,
    entryCount: lines.length,
  };
}
