// A client-side SWIFT FIN tokenizer. It splits a raw FIN message into its five
// blocks and decomposes block 4 into :NN: tag fields and :16R:/:16S: sequence
// groups — with source character spans so the Parser workbench can hover-sync
// the raw text against the structured tree. This is a *structural* splitter
// (mechanically simple), NOT the authoritative schema validator: swift-core /
// swift-schema on the backend remain the source of truth for validation; this
// gives instant offline structure + spans that backend facts overlay onto.

export interface Span {
  start: number;
  end: number;
}

export type FieldKind = "field" | "seqStart" | "seqEnd";

export interface FinField {
  /** Tag without colons, e.g. "20", "35B", "61", "16R". */
  tag: string;
  /** Raw value text (may span multiple lines). */
  value: string;
  lines: string[];
  /** Enclosing :16R: sequence stack at this point, e.g. ["TRADDET","FIAC"]. */
  seqPath: string[];
  kind: FieldKind;
  /** For seq markers, the sequence name (the value, e.g. "GENL"). */
  seqName?: string;
  /** Span of the whole ":tag:value" run in the raw source. */
  span: Span;
  /** Span of just the value in the raw source. */
  valueSpan: Span;
}

export interface HeaderKV {
  label: string;
  value: string;
  hint?: string;
}

export interface FinBlock {
  id: string; // "1".."5"
  label: string;
  raw: string;
  span: Span;
  /** Decoded key/values for header blocks 1/2. */
  kv?: HeaderKV[];
  /** Parsed tag fields for block 4. */
  fields?: FinField[];
}

export interface FinMessage {
  raw: string;
  blocks: FinBlock[];
  fields: FinField[]; // block 4, flat
  messageType: string | null; // "MT940"
  direction: "input" | "output" | null;
  senderBic: string | null;
  receiverBic: string | null;
  error: string | null;
}

const BLOCK_LABEL: Record<string, string> = {
  "1": "Basic Header",
  "2": "Application Header",
  "3": "User Header",
  "4": "Text Block",
  "5": "Trailer",
};

/** Split a raw FIN string into top-level {n:...} blocks (brace-depth aware, so
 *  nested {tag:...} subfields in blocks 3/5 don't confuse the terminator). */
function splitBlocks(raw: string): FinBlock[] {
  const blocks: FinBlock[] = [];
  let i = 0;
  const n = raw.length;
  while (i < n) {
    if (raw[i] !== "{") {
      i++;
      continue;
    }
    const start = i;
    const colon = raw.indexOf(":", i);
    if (colon < 0) break;
    const id = raw.slice(i + 1, colon);
    // Find the matching close brace from `colon`, tracking nesting.
    let depth = 1;
    let j = colon + 1;
    for (; j < n; j++) {
      if (raw[j] === "{") depth++;
      else if (raw[j] === "}") {
        depth--;
        if (depth === 0) break;
      }
    }
    const end = Math.min(j + 1, n);
    const inner = raw.slice(colon + 1, j);
    blocks.push({
      id,
      label: BLOCK_LABEL[id] ?? `Block ${id}`,
      raw: inner,
      span: { start, end },
      kv: id === "1" || id === "2" ? decodeHeader(id, inner) : undefined,
    });
    i = end;
  }
  return blocks;
}

function decodeHeader(id: string, inner: string): HeaderKV[] {
  if (id === "1") {
    // e.g. F01BANKGB22AXXX0000000000
    const appId = inner.slice(0, 1);
    const service = inner.slice(1, 3);
    const lt = inner.slice(3, 15);
    const session = inner.slice(15, 19);
    const sequence = inner.slice(19, 25);
    return [
      { label: "Application", value: appId, hint: appId === "F" ? "FIN" : appId },
      { label: "Service", value: service },
      { label: "LT address", value: lt, hint: "Sender BIC + LT + branch" },
      { label: "Session", value: session },
      { label: "Sequence", value: sequence },
    ].filter((k) => k.value);
  }
  // Block 2 — input `I<mt><bic><prio>` or output `O<mt>...`.
  const io = inner.slice(0, 1);
  if (io === "I") {
    const mt = inner.slice(1, 4);
    const address = inner.slice(4, 16);
    const priority = inner.slice(16, 17);
    return [
      { label: "Direction", value: "Input", hint: "I" },
      { label: "Message type", value: `MT${mt}` },
      { label: "Receiver", value: address, hint: "Destination BIC" },
      { label: "Priority", value: priority || "—" },
    ];
  }
  if (io === "O") {
    const mt = inner.slice(1, 4);
    return [
      { label: "Direction", value: "Output", hint: "O" },
      { label: "Message type", value: `MT${mt}` },
      { label: "Header", value: inner.slice(4), hint: "Input time / MIR / output date" },
    ];
  }
  return [{ label: "Raw", value: inner }];
}

const TAG_RE = /^:(\d{2}[A-Z]?):(.*)$/;

/** Parse block-4 inner text into fields with absolute source spans.
 *  `base` is the offset of `inner` within the original raw string. */
function parseFields(inner: string, base: number): FinField[] {
  const fields: FinField[] = [];
  const stack: string[] = [];
  // Walk lines, tracking absolute offsets.
  let offset = 0;
  const lines = inner.split("\n");
  let current: FinField | null = null;
  const commit = () => {
    if (!current) return;
    current.value = current.lines.join("\n");
    current.valueSpan.end = current.span.end;
    fields.push(current);
    current = null;
  };
  for (let li = 0; li < lines.length; li++) {
    const line = lines[li];
    const lineStart = base + offset;
    const lineEnd = lineStart + line.length;
    offset += line.length + 1; // +1 for the split '\n'
    const trimmed = line.trim();
    if (trimmed === "-" || trimmed === "") {
      // Block-4 terminator / blank — closes any open multi-line field.
      if (trimmed === "-") commit();
      continue;
    }
    const m = line.match(TAG_RE);
    if (m) {
      commit();
      const tag = m[1];
      const valStart = lineStart + m[0].length - m[2].length;
      const isSeq = tag === "16R" || tag === "16S";
      const seqPathBefore = [...stack];
      if (tag === "16R") stack.push(m[2].trim());
      const seqPath = tag === "16R" ? seqPathBefore : tag === "16S" ? seqPathBefore : [...stack];
      current = {
        tag,
        value: m[2],
        lines: [m[2]],
        seqPath,
        kind: isSeq ? (tag === "16R" ? "seqStart" : "seqEnd") : "field",
        seqName: isSeq ? m[2].trim() : undefined,
        span: { start: lineStart, end: lineEnd },
        valueSpan: { start: valStart, end: lineEnd },
      };
      if (tag === "16S") stack.pop();
    } else if (current) {
      // Continuation of a multi-line value (e.g. :35B: instrument description).
      current.lines.push(line);
      current.span.end = lineEnd;
    }
  }
  commit();
  return fields;
}

export function parseFin(raw: string): FinMessage {
  const blocks = splitBlocks(raw);
  const block4 = blocks.find((b) => b.id === "4");
  if (block4) {
    // inner begins right after "{4:" — its offset in raw:
    const innerBase = block4.span.start + 1 + block4.id.length + 1;
    block4.fields = parseFields(block4.raw, innerBase);
  }
  const fields = block4?.fields ?? [];

  const b1 = blocks.find((b) => b.id === "1");
  const b2 = blocks.find((b) => b.id === "2");
  const mtKv = b2?.kv?.find((k) => k.label === "Message type");
  const messageType = mtKv?.value ?? null;
  const dirKv = b2?.kv?.find((k) => k.label === "Direction");
  const direction = dirKv ? (dirKv.value === "Input" ? "input" : "output") : null;
  const lt = b1?.kv?.find((k) => k.label === "LT address")?.value ?? null;
  const senderBic = lt ? lt.slice(0, 8) : null;
  const receiverRaw = b2?.kv?.find((k) => k.label === "Receiver")?.value ?? null;
  const receiverBic = receiverRaw ? receiverRaw.slice(0, 8) : null;

  const error =
    blocks.length === 0
      ? "No FIN blocks found. A message looks like {1:…}{2:…}{4:…-}."
      : !block4
        ? "No text block (block 4) — nothing to parse into fields."
        : null;

  return { raw, blocks, fields, messageType, direction, senderBic, receiverBic, error };
}
