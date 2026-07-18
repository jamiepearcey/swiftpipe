import { useEffect, useState } from "react";
import { listJobs } from "../api";
import type { JobStatus, JobStatusView } from "../types";

interface Props {
  selectedJobId: string | null;
  onSelect: (jobId: string) => void;
}

function statusClass(s: JobStatus): string {
  if (s === "completed") return "tag tag-ok";
  if (s === "completed_with_errors") return "tag tag-warn";
  if (s === "failed") return "tag tag-bad";
  if (s === "running") return "tag tag-run";
  return "tag tag-muted";
}

function fmtAge(iso: string): string {
  const diff = Math.round((Date.now() - new Date(iso).getTime()) / 1000);
  if (diff < 60) return `${diff}s ago`;
  if (diff < 3600) return `${Math.round(diff / 60)}m ago`;
  return `${Math.round(diff / 3600)}h ago`;
}

export default function JobsList({ selectedJobId, onSelect }: Props) {
  const [jobs, setJobs] = useState<JobStatusView[]>([]);

  useEffect(() => {
    let active = true;
    async function refresh() {
      try {
        const list = await listJobs(50);
        if (active) setJobs(list);
      } catch {
        // silent — show stale list
      }
    }
    refresh();
    const id = setInterval(refresh, 2_000);
    return () => {
      active = false;
      clearInterval(id);
    };
  }, []);

  if (jobs.length === 0) {
    return (
      <div className="jobs-empty">No jobs yet. Submit a FIN above to start.</div>
    );
  }

  return (
    <div className="jobs-table-wrap">
      <table className="jobs-table">
        <thead>
          <tr>
            <th>Job ID</th>
            <th>Status</th>
            <th>Age</th>
          </tr>
        </thead>
        <tbody>
          {jobs.map((j) => (
            <tr
              key={j.job_id}
              className={j.job_id === selectedJobId ? "selected" : ""}
              onClick={() => onSelect(j.job_id)}
            >
              <td className="job-id-cell">
                <code>{j.job_id.slice(0, 8)}…</code>
              </td>
              <td>
                <span className={statusClass(j.status)}>{j.status}</span>
              </td>
              <td className="age-cell">{fmtAge(j.created_at)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
