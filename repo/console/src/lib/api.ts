// Unified client for the two swiftpipe surfaces. One fetch wrapper injects the
// bearer token for swift-api and reports 401/403 to the shared auth bus; the
// recon read-model is unauthenticated and degrades to a static snapshot.
import { getSettings, apiToken } from "@/lib/settings";
import { reportAuthFailure, clearAuthFailure } from "@/lib/auth";
import {
  isTerminal,
  type CsdrSnapshot,
  type JobManifest,
  type JobRequest,
  type JobStatus,
  type JobStatusView,
  type Metrics,
  type ReconSnapshot,
  type SubmitResponse,
} from "@/lib/types";

function apiUrl(path: string): string {
  return `${getSettings().connection.apiBase}${path}`;
}

async function nfetch(path: string, init?: RequestInit): Promise<Response> {
  const token = apiToken();
  const headers = new Headers(init?.headers);
  if (token) headers.set("Authorization", `Bearer ${token}`);
  const res = await fetch(apiUrl(path), { ...init, headers });
  if (res.status === 401 || res.status === 403) reportAuthFailure(res.status);
  else clearAuthFailure();
  return res;
}

async function json<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await nfetch(path, init);
  const body = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error((body as { error?: string }).error ?? res.statusText);
  return body as T;
}

// ---- swift-api ------------------------------------------------------------
export async function health(): Promise<boolean> {
  try {
    const res = await nfetch("/healthz");
    return res.ok;
  } catch {
    return false;
  }
}

export async function submitUpload(
  fin: string,
  messageType?: string,
  outputs?: string,
): Promise<SubmitResponse> {
  const params = new URLSearchParams();
  if (messageType) params.set("message_type", messageType);
  if (outputs) params.set("outputs", outputs);
  const qs = params.toString();
  return json<SubmitResponse>(`/v1/upload${qs ? `?${qs}` : ""}`, { method: "POST", body: fin });
}

export async function submitJob(req: JobRequest): Promise<SubmitResponse> {
  return json<SubmitResponse>("/v1/jobs", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(req),
  });
}

export const getJobStatus = (jobId: string) =>
  json<JobStatusView>(`/v1/jobs/${encodeURIComponent(jobId)}`);

export const listJobs = (limit = 50) =>
  json<JobStatusView[]>(`/v1/jobs?limit=${limit}`);

export const getManifest = (jobId: string) =>
  json<JobManifest>(`/v1/jobs/${encodeURIComponent(jobId)}/manifest`);

export const getMetrics = () => json<Metrics>("/metrics/json");

export async function getObject(uri: string): Promise<string> {
  const res = await nfetch(`/v1/object/${encodeURIComponent(uri)}`);
  if (!res.ok) throw new Error(`Failed to fetch object: ${res.statusText}`);
  return res.text();
}

/** Poll a job to a terminal state, invoking `onProgress` at each tick. */
export async function pollJob(
  jobId: string,
  onProgress?: (status: JobStatus) => void,
  { intervalMs = 600, timeoutMs = 120_000 }: { intervalMs?: number; timeoutMs?: number } = {},
): Promise<JobStatusView> {
  const deadline = Date.now() + timeoutMs;
  // Fast first check, then backoff toward ~1.5s.
  let delay = intervalMs;
  while (Date.now() < deadline) {
    const view = await getJobStatus(jobId);
    onProgress?.(view.status);
    if (isTerminal(view.status)) return view;
    await new Promise((r) => setTimeout(r, delay));
    delay = Math.min(delay + 200, 1500);
  }
  throw new Error("Timed out waiting for job to complete");
}

// ---- recon read-model (Surface B) -----------------------------------------
export interface ReconResult {
  snapshot: ReconSnapshot;
  /** True when served from the static fallback rather than the live service. */
  stale: boolean;
}

/** Fetch the recon snapshot; fall back to the static JSON when the read-model
 *  service is unreachable so the Books desk still renders (Fable spec §4). */
export async function getReconSnapshot(): Promise<ReconResult> {
  const { reconBase, reconFallback } = getSettings().connection;
  try {
    const res = await fetch(`${reconBase}/recon/snapshot`);
    if (!res.ok) throw new Error(String(res.status));
    return { snapshot: (await res.json()) as ReconSnapshot, stale: false };
  } catch {
    const res = await fetch(reconFallback);
    if (!res.ok) throw new Error("recon read-model unreachable and no static snapshot available");
    return { snapshot: (await res.json()) as ReconSnapshot, stale: true };
  }
}

export async function reconHealth(): Promise<boolean> {
  try {
    const res = await fetch(`${getSettings().connection.reconBase}/recon/snapshot`, { method: "HEAD" });
    return res.ok;
  } catch {
    return false;
  }
}

// ---- CSDR penalty read-model (Surface B) ----------------------------------
export interface CsdrResult {
  snapshot: CsdrSnapshot;
  stale: boolean;
}

/** Fetch the CSDR penalty snapshot; fall back to a static JSON when the
 *  read-model is unreachable, mirroring `getReconSnapshot`. */
export async function getCsdrSnapshot(): Promise<CsdrResult> {
  const { csdrBase, csdrFallback } = getSettings().connection;
  try {
    const res = await fetch(`${csdrBase}/csdr/snapshot`);
    if (!res.ok) throw new Error(String(res.status));
    return { snapshot: (await res.json()) as CsdrSnapshot, stale: false };
  } catch {
    const res = await fetch(csdrFallback);
    if (!res.ok) throw new Error("CSDR read-model unreachable and no static snapshot available");
    return { snapshot: (await res.json()) as CsdrSnapshot, stale: true };
  }
}

export async function csdrHealth(): Promise<boolean> {
  try {
    const res = await fetch(`${getSettings().connection.csdrBase}/csdr/snapshot`, { method: "HEAD" });
    return res.ok;
  } catch {
    return false;
  }
}
