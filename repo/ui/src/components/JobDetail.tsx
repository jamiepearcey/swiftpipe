import { useEffect, useState } from "react";
import { getManifest, getObject } from "../api";
import type { JobManifest, JobStatusView } from "../types";

interface Props {
  jobId: string;
  statusView: JobStatusView | null;
}

export default function JobDetail({ jobId, statusView }: Props) {
  const [manifest, setManifest] = useState<JobManifest | null>(null);
  const [rendered, setRendered] = useState<string | null>(null);
  const [manifestError, setManifestError] = useState<string | null>(null);

  const isTerminal =
    statusView?.status === "completed" ||
    statusView?.status === "completed_with_errors" ||
    statusView?.status === "failed";

  useEffect(() => {
    setManifest(null);
    setRendered(null);
    setManifestError(null);
    if (!isTerminal) return;
    let active = true;

    async function load() {
      try {
        const m = await getManifest(jobId);
        if (!active) return;
        setManifest(m);
        const renderedUri = m.messages?.find((msg) => msg.rendered_uri)
          ?.rendered_uri;
        if (renderedUri) {
          try {
            const text = await getObject(renderedUri);
            if (active) setRendered(text);
          } catch {
            // rendered FIN optional
          }
        }
      } catch (e) {
        if (active)
          setManifestError(e instanceof Error ? e.message : String(e));
      }
    }

    load();
    return () => {
      active = false;
    };
  }, [jobId, isTerminal]);

  const status = statusView?.status ?? "…";

  function statusClass(): string {
    if (status === "completed") return "status-ok";
    if (status === "completed_with_errors") return "status-warn";
    if (status === "failed") return "status-bad";
    if (status === "running") return "status-run";
    return "status-muted";
  }

  return (
    <div className="job-detail">
      <div className={`job-status-banner ${statusClass()}`}>
        <span>
          <strong>Job:</strong> <code>{jobId}</code>
        </span>
        <span className="job-status-badge">{status}</span>
      </div>

      {statusView?.error && (
        <div className="job-error-box">{statusView.error}</div>
      )}

      {manifest && (
        <div className="detail-counts">
          <span>
            <strong>{manifest.counts.messages}</strong> messages
          </span>
          <span>
            <strong>{manifest.counts.rendered}</strong> rendered
          </span>
          <span>
            <strong>{manifest.counts.errors}</strong> errors
          </span>
          {manifest.timings.total_ms != null && (
            <span>{manifest.timings.total_ms}ms</span>
          )}
        </div>
      )}

      {manifest?.outputs && (
        <div className="artifact-links">
          {Object.entries(manifest.outputs)
            .filter(([, uri]) => uri)
            .map(([key, uri]) => (
              <a
                key={key}
                href={`/v1/object/${encodeURIComponent(uri!)}`}
                target="_blank"
                className="artifact-link"
              >
                {key}
              </a>
            ))}
        </div>
      )}

      <div className="detail-panels">
        <div className="detail-panel">
          <div className="panel-title">Manifest</div>
          <pre className="code-block">
            {manifestError
              ? `Error: ${manifestError}`
              : manifest
                ? JSON.stringify(manifest, null, 2)
                : !isTerminal
                  ? `Status: ${status}\nWaiting for job to complete…`
                  : "Loading…"}
          </pre>
        </div>

        <div className="detail-panel">
          <div className="panel-title">Rendered FIN</div>
          <pre className="code-block">
            {rendered ?? (manifest ? "(no rendered output)" : "")}
          </pre>
        </div>
      </div>
    </div>
  );
}
