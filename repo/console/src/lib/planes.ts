import type { ViewId } from "@/lib/route";

/** Product planes — each view belongs to exactly one; used for IA + chrome. */
export type PlaneId = "pipeline" | "parser" | "books" | "control" | "regulatory" | "system";

export interface PlaneDef {
  id: PlaneId;
  label: string;
  job: string;
}

export const PLANES: Record<PlaneId, PlaneDef> = {
  pipeline: { id: "pipeline", label: "Pipeline", job: "Ingest jobs, throughput, artifacts" },
  parser: { id: "parser", label: "Parser", job: "Dissect a SWIFT message end to end" },
  books: { id: "books", label: "Books", job: "Reconciliation, cash P&L, positions" },
  control: { id: "control", label: "Control", job: "Sources and the schema catalog" },
  regulatory: { id: "regulatory", label: "Regulatory", job: "CSDR settlement penalties and recon" },
  system: { id: "system", label: "System", job: "Session activity and settings" },
};

export const VIEW_PLANE: Record<ViewId, PlaneId> = {
  overview: "pipeline",
  jobs: "pipeline",
  parser: "parser",
  recon: "books",
  positions: "books",
  csdr: "regulatory",
  sources: "control",
  schemas: "control",
  activity: "system",
  settings: "system",
};
