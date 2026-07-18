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
