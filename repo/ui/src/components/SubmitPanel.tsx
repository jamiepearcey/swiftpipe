import { useRef, useState } from "react";
import { pollJob, submitJob, submitUpload } from "../api";
import type { JobStatus } from "../types";

interface Props {
  onJobStarted: (jobId: string) => void;
}

type Tab = "upload" | "prefix";

export default function SubmitPanel({ onJobStarted }: Props) {
  const [tab, setTab] = useState<Tab>("upload");
  const [busy, setBusy] = useState(false);
  const [submitStatus, setSubmitStatus] = useState<string>("");
  const [submitError, setSubmitError] = useState<string>("");

  // Upload fields
  const [fin, setFin] = useState("");
  const [messageType, setMessageType] = useState("");
  const [uploadOutputs, setUploadOutputs] = useState("all");
  const fileRef = useRef<HTMLInputElement>(null);

  // Prefix job fields
  const [inputPrefix, setInputPrefix] = useState("");
  const [includeSuffix, setIncludeSuffix] = useState(".fin");
  const [prefixMessageType, setPrefixMessageType] = useState("");
  const [prefixOutputs, setPrefixOutputs] = useState("all");

  function track(status: JobStatus) {
    setSubmitStatus(`Job ${status}…`);
  }

  async function handleUpload() {
    if (!fin.trim()) {
      setSubmitError("Paste or upload a FIN message first");
      return;
    }
    setBusy(true);
    setSubmitError("");
    setSubmitStatus("Submitting…");
    try {
      const outputsParam =
        uploadOutputs === "all" ? undefined : uploadOutputs;
      const resp = await submitUpload(
        fin,
        messageType.trim() || undefined,
        outputsParam
      );
      setSubmitStatus(`Queued as ${resp.job_id}`);
      onJobStarted(resp.job_id);
      await pollJob(resp.job_id, track);
      setSubmitStatus("Done");
    } catch (e) {
      setSubmitError(e instanceof Error ? e.message : String(e));
      setSubmitStatus("");
    } finally {
      setBusy(false);
    }
  }

  async function handlePrefix() {
    if (!inputPrefix.trim()) {
      setSubmitError("Input prefix is required");
      return;
    }
    setBusy(true);
    setSubmitError("");
    setSubmitStatus("Submitting…");
    try {
      const resp = await submitJob({
        input_prefix: inputPrefix.trim(),
        include_suffix: includeSuffix.trim() || ".fin",
        message_type: prefixMessageType.trim() || undefined,
        render_validate: true,
        outputs:
          prefixOutputs === "all"
            ? undefined
            : prefixOutputs.split(",").map((s) => s.trim()),
      });
      setSubmitStatus(`Queued as ${resp.job_id}`);
      onJobStarted(resp.job_id);
      await pollJob(resp.job_id, track);
      setSubmitStatus("Done");
    } catch (e) {
      setSubmitError(e instanceof Error ? e.message : String(e));
      setSubmitStatus("");
    } finally {
      setBusy(false);
    }
  }

  async function handleFileChange(e: React.ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0];
    if (file) setFin(await file.text());
  }

  return (
    <div className="submit-panel">
      <div className="tabs" role="tablist">
        <button
          role="tab"
          aria-selected={tab === "upload"}
          onClick={() => setTab("upload")}
          disabled={busy}
        >
          Upload FIN
        </button>
        <button
          role="tab"
          aria-selected={tab === "prefix"}
          onClick={() => setTab("prefix")}
          disabled={busy}
        >
          Prefix Job
        </button>
      </div>

      {tab === "upload" && (
        <div className="tab-panel stack">
          <label className="field-label">
            FIN file
            <input
              ref={fileRef}
              type="file"
              accept=".fin,.txt"
              onChange={handleFileChange}
            />
          </label>
          <label className="field-label">
            Message type
            <input
              type="text"
              placeholder="MT540 (auto-detected if blank)"
              value={messageType}
              onChange={(e) => setMessageType(e.target.value)}
            />
          </label>
          <label className="field-label">
            Outputs
            <select
              value={uploadOutputs}
              onChange={(e) => setUploadOutputs(e.target.value)}
            >
              <option value="all">Full artifacts</option>
              <option value="rendered">Rendered FIN only</option>
              <option value="parquet,errors">Parquet + errors</option>
            </select>
          </label>
          <label className="field-label">
            FIN message
            <textarea
              rows={10}
              placeholder="{1:...}{2:...}{4:...-}"
              value={fin}
              onChange={(e) => setFin(e.target.value)}
            />
          </label>
          <button
            className="primary-btn"
            onClick={handleUpload}
            disabled={busy}
          >
            {busy ? "Processing…" : "Upload and process"}
          </button>
        </div>
      )}

      {tab === "prefix" && (
        <div className="tab-panel stack">
          <label className="field-label">
            Input prefix
            <input
              type="text"
              placeholder="s3://swiftpipe-inbox/daily/"
              value={inputPrefix}
              onChange={(e) => setInputPrefix(e.target.value)}
            />
          </label>
          <div className="row-2">
            <label className="field-label">
              Include suffix
              <input
                type="text"
                value={includeSuffix}
                onChange={(e) => setIncludeSuffix(e.target.value)}
              />
            </label>
            <label className="field-label">
              Message type
              <input
                type="text"
                placeholder="MT540 (optional)"
                value={prefixMessageType}
                onChange={(e) => setPrefixMessageType(e.target.value)}
              />
            </label>
          </div>
          <label className="field-label">
            Outputs
            <select
              value={prefixOutputs}
              onChange={(e) => setPrefixOutputs(e.target.value)}
            >
              <option value="all">Full artifacts</option>
              <option value="rendered">Rendered FIN only</option>
              <option value="parquet,errors">Parquet + errors</option>
            </select>
          </label>
          <button
            className="primary-btn"
            onClick={handlePrefix}
            disabled={busy}
          >
            {busy ? "Processing…" : "Run prefix job"}
          </button>
        </div>
      )}

      {submitStatus && (
        <div className="submit-status">{submitStatus}</div>
      )}
      {submitError && (
        <div className="submit-error">{submitError}</div>
      )}
    </div>
  );
}
