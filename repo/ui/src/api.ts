import type {
  JobManifest,
  JobRequest,
  JobStatus,
  JobStatusView,
  Metrics,
  SubmitResponse,
} from "./types";

const API_BASE = import.meta.env.VITE_API_BASE ?? "";

function authHeaders(): Record<string, string> {
  const token = localStorage.getItem("swiftpipe_token");
  return token ? { Authorization: `Bearer ${token}` } : {};
}

async function apiFetch<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    ...init,
    headers: {
      ...authHeaders(),
      ...(init?.headers ?? {}),
    },
  });
  const body = await res.json();
  if (!res.ok) {
    throw new Error((body as { error?: string }).error ?? res.statusText);
  }
  return body as T;
}

export async function submitUpload(
  fin: string,
  messageType?: string,
  outputs?: string
): Promise<SubmitResponse> {
  const params = new URLSearchParams();
  if (messageType) params.set("message_type", messageType);
  if (outputs) params.set("outputs", outputs);
  const qs = params.toString();
  return apiFetch<SubmitResponse>(`/v1/upload${qs ? `?${qs}` : ""}`, {
    method: "POST",
    body: fin,
  });
}

export async function submitJob(req: JobRequest): Promise<SubmitResponse> {
  return apiFetch<SubmitResponse>("/v1/jobs", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(req),
  });
}

export async function getJobStatus(jobId: string): Promise<JobStatusView> {
  return apiFetch<JobStatusView>(`/v1/jobs/${jobId}`);
}

export async function listJobs(limit = 50): Promise<JobStatusView[]> {
  return apiFetch<JobStatusView[]>(`/v1/jobs?limit=${limit}`);
}

export async function getManifest(jobId: string): Promise<JobManifest> {
  return apiFetch<JobManifest>(`/v1/jobs/${jobId}/manifest`);
}

export async function getMetrics(): Promise<Metrics> {
  return apiFetch<Metrics>("/metrics/json");
}

export async function getObject(uri: string): Promise<string> {
  const res = await fetch(`${API_BASE}/v1/object/${encodeURIComponent(uri)}`, {
    headers: authHeaders(),
  });
  if (!res.ok) throw new Error(`Failed to fetch object: ${res.statusText}`);
  return res.text();
}

export function isTerminal(status: JobStatus): boolean {
  return (
    status === "completed" ||
    status === "completed_with_errors" ||
    status === "failed"
  );
}

export async function pollJob(
  jobId: string,
  onProgress: (status: JobStatus) => void,
  intervalMs = 1000,
  timeoutMs = 600_000
): Promise<JobStatusView> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, intervalMs));
    const view = await getJobStatus(jobId);
    onProgress(view.status);
    if (isTerminal(view.status)) return view;
  }
  throw new Error("Timed out waiting for job to complete");
}
