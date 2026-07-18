import { useEffect, useState } from "react";

interface HeaderProps {
  token: string;
  onTokenChange: (t: string) => void;
}

export default function Header({ token, onTokenChange }: HeaderProps) {
  const [health, setHealth] = useState<"ok" | "error" | "checking">(
    "checking"
  );

  useEffect(() => {
    let active = true;
    async function check() {
      try {
        const res = await fetch("/healthz");
        if (!active) return;
        setHealth(res.ok ? "ok" : "error");
      } catch {
        if (active) setHealth("error");
      }
    }
    check();
    const id = setInterval(check, 15_000);
    return () => {
      active = false;
      clearInterval(id);
    };
  }, []);

  const dot =
    health === "ok" ? "●" : health === "error" ? "●" : "○";
  return (
    <header className="app-header">
      <div className="header-left">
        <span className="logo">SwiftPipe</span>
        <span className={`health health-${health}`} title={`API ${health}`}>
          {dot} {health}
        </span>
      </div>
      <div className="header-right">
        <input
          className="token-input"
          type="password"
          placeholder="API token (optional)"
          value={token}
          onChange={(e) => onTokenChange(e.target.value)}
        />
        <a href="/docs" target="_blank" className="header-link">
          API Docs
        </a>
        <a href="/openapi.json" target="_blank" className="header-link">
          OpenAPI
        </a>
      </div>
    </header>
  );
}
