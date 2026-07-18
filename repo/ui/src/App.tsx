import { useEffect, useState } from "react";
import "./App.css";
import Header from "./components/Header";
import JobDetail from "./components/JobDetail";
import JobsList from "./components/JobsList";
import MetricsBar from "./components/MetricsBar";
import SubmitPanel from "./components/SubmitPanel";
import { getJobStatus } from "./api";
import type { JobStatusView } from "./types";

export default function App() {
  const [token, setToken] = useState(
    () => localStorage.getItem("swiftpipe_token") ?? ""
  );
  const [selectedJobId, setSelectedJobId] = useState<string | null>(null);
  const [selectedView, setSelectedView] = useState<JobStatusView | null>(null);

  function handleTokenChange(t: string) {
    setToken(t);
    if (t) localStorage.setItem("swiftpipe_token", t);
    else localStorage.removeItem("swiftpipe_token");
  }

  function handleJobStarted(jobId: string) {
    setSelectedJobId(jobId);
  }

  // Poll selected job status for live banner updates
  useEffect(() => {
    if (!selectedJobId) return;
    let active = true;

    async function pollView() {
      try {
        const view = await getJobStatus(selectedJobId!);
        if (!active) return;
        setSelectedView(view);
        const terminal =
          view.status === "completed" ||
          view.status === "completed_with_errors" ||
          view.status === "failed";
        if (!terminal) {
          setTimeout(pollView, 1000);
        }
      } catch {
        if (active) setTimeout(pollView, 3000);
      }
    }

    setSelectedView(null);
    pollView();
    return () => {
      active = false;
    };
  }, [selectedJobId]);

  return (
    <>
      <Header token={token} onTokenChange={handleTokenChange} />
      <MetricsBar />
      <div className="app-body">
        <div className="left-col">
          <SubmitPanel onJobStarted={handleJobStarted} />
          <div className="jobs-section">
            <div className="jobs-section-title">Recent Jobs</div>
            <JobsList
              selectedJobId={selectedJobId}
              onSelect={(id) => setSelectedJobId(id)}
            />
          </div>
        </div>
        <div className="right-col">
          {selectedJobId ? (
            <JobDetail jobId={selectedJobId} statusView={selectedView} />
          ) : (
            <div className="no-selection">
              Select a job or submit one to see details
            </div>
          )}
        </div>
      </div>
    </>
  );
}
