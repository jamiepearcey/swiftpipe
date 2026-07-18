import { useEffect, useState } from "react";
import { getMetrics } from "../api";
import type { Metrics } from "../types";

export default function MetricsBar() {
  const [metrics, setMetrics] = useState<Metrics | null>(null);
  const [error, setError] = useState(false);

  useEffect(() => {
    let active = true;
    async function refresh() {
      try {
        const m = await getMetrics();
        if (active) {
          setMetrics(m);
          setError(false);
        }
      } catch {
        if (active) setError(true);
      }
    }
    refresh();
    const id = setInterval(refresh, 5_000);
    return () => {
      active = false;
      clearInterval(id);
    };
  }, []);

  if (error) {
    return <div className="metrics-bar metrics-error">metrics unavailable</div>;
  }

  if (!metrics) {
    return <div className="metrics-bar metrics-loading">loading metrics…</div>;
  }

  const items: [string, number][] = [
    ["Submitted", metrics.jobs_submitted],
    ["Completed", metrics.jobs_completed],
    ["Failed", metrics.jobs_failed],
    ["In flight", metrics.jobs_in_flight],
  ];

  return (
    <div className="metrics-bar">
      {items.map(([label, value]) => (
        <div key={label} className="metric-cell">
          <span className="metric-value">{value}</span>
          <span className="metric-label">{label}</span>
        </div>
      ))}
      <a href="/metrics" target="_blank" className="metrics-link">
        Prometheus
      </a>
    </div>
  );
}
