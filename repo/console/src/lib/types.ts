// Typed contracts for the two swiftpipe HTTP surfaces + the build-time assets.
// Hand-written from the Rust sources (swift-api job manifest, ingest-cli
// ReconSnapshot, examples/schemas) — 15 types, not worth an OpenAPI toolchain.

// ---- Surface A: swift-api (jobs / upload / manifest / metrics) -------------
export type JobStatus =
  | "queued"
  | "running"
  | "completed"
  | "completed_with_errors"
  | "failed";

export interface JobStatusView {
  job_id: string;
  status: JobStatus;
  created_at: string;
  updated_at: string;
  error?: string | null;
}

export interface MessageEntry {
  message_id?: string;
  message_type?: string;
  rendered_uri?: string;
  error?: string;
}

export interface JobCounts {
  messages: number;
  rendered: number;
  errors: number;
}

export interface JobTimings {
  parse_ms?: number;
  render_ms?: number;
  export_ms?: number;
  zip_ms?: number;
  total_ms?: number;
}

export interface JobOutputs {
  manifest?: string;
  errors_ndjson?: string;
  zip?: string;
  [key: string]: string | undefined;
}

export interface JobManifest {
  contract_version: string;
  job_id: string;
  status: JobStatus;
  counts: JobCounts;
  outputs: JobOutputs;
  timings: JobTimings;
  messages?: MessageEntry[];
}

export interface SubmitResponse {
  job_id: string;
  status: string;
  poll_url: string;
}

export interface JobRequest {
  input_prefix: string;
  include_suffix?: string;
  message_type?: string;
  render_validate?: boolean;
  outputs?: string[];
}

export interface Metrics {
  jobs_submitted: number;
  jobs_completed: number;
  jobs_failed: number;
  jobs_in_flight: number;
  queue_depth: number;
}

export function isTerminal(status: JobStatus): boolean {
  return status === "completed" || status === "completed_with_errors" || status === "failed";
}

// ---- Surface B: ingest serve (recon read-model) ---------------------------
export interface SnapshotEntry {
  valueDate: string;
  direction: string; // credit | debit | reversal_credit | reversal_debit
  amount: number;
  signedAmount: number;
  transactionType: string; // TRF | CHG | DIV | INT | ...
  reference?: string;
  info?: string;
}

export interface SnapshotStatement {
  source: string;
  messageId: string;
  messageType: string;
  account: string;
  currency: string;
  opening: number;
  closing: number;
  entries: SnapshotEntry[];
}

export interface SnapshotPosition {
  source: string;
  messageType: string;
  isin: string;
  desc?: string;
  quantity: number;
  safekeepingAccount?: string;
}

export interface ReconSnapshot {
  version: number;
  statements: SnapshotStatement[];
  positions: SnapshotPosition[];
}

// ---- CSDR penalty read-model (GET /csdr/snapshot) -------------------------
export interface CsdrAccrual {
  source: string;
  transactionRef: string;
  isin: string;
  instrumentDesc?: string;
  instrumentType: string; // liquid_share | sovereign_bond | corporate_bond | ...
  counterparty?: string;
  currency: string;
  quantity?: number;
  referenceAmount: number;
  penaltyType: string; // SEFP | LMFP
  penaltyRateBps: number;
  status: string; // PEND | PENF
  intendedSettlementDate?: string;
  computedAmount: number;
  direction: string; // payable | receivable
}

export type CsdrReconStatus = "matched" | "break" | "missing_reported" | "missing_computed";

export interface CsdrReconLine {
  transactionRef: string;
  isin: string;
  counterparty?: string;
  currency: string;
  penaltyType: string;
  computed: number;
  reported: number;
  diff: number;
  status: CsdrReconStatus;
}

export interface CsdrSummary {
  computedTotal: number;
  reportedTotal: number;
  netDiff: number;
  accruals: number;
  reported: number;
  matched: number;
  breaks: number;
  missingReported: number;
  missingComputed: number;
  breakAmount: number;
  allReconciled: boolean;
}

export interface CsdrSnapshot {
  version: number;
  summary: CsdrSummary;
  accruals: CsdrAccrual[];
  lines: CsdrReconLine[];
}

// ---- Build-time assets (scripts/gen-assets.mjs) ---------------------------
export interface SchemaField {
  path: string;
  tag: string;
  qualifier: string | null;
  name: string;
  type: string;
  required: boolean;
  entity: string;
  column: string;
  options: string[] | null;
}

export interface SchemaCoverage {
  source?: string;
  exact?: boolean;
  expected_sequences?: number;
  expected_fields?: number;
  notes?: string[];
}

export interface SchemaDef {
  mt: string;
  category: string;
  version: string;
  coverage: SchemaCoverage | null;
  sequences: string[];
  fields: SchemaField[];
}

export interface SampleDef {
  id: string;
  mt: string;
  label: string;
  category: string;
  fin: string;
}
