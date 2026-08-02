// System settings — wire up the console to swift-api and the recon read-model,
// and pick a theme. Connection fields are controlled inputs over the shared
// settings store; "Test connection" probes both backends on demand.
import { useState } from "react";
import { PlaneHeader } from "@/components/PlaneHeader";
import { NodeErrorBanner } from "@/components/NodeErrorBanner";
import { StatusBadge } from "@/components/StatusBadge";
import { Btn, Card, Field, Segmented, errText, inputCls } from "@/components/setup/kit";
import { health, reconHealth } from "@/lib/api";
import { patchConnection, setTheme, useSettings } from "@/lib/settings";
import type { AppRoute } from "@/lib/route";
import { PlugZap } from "lucide-react";

type TestResult = {
  api: boolean | null;
  recon: boolean | null;
  error: string | null;
};

const TABS = [
  { id: "connection" as const, label: "Connection" },
  { id: "appearance" as const, label: "Appearance" },
];

export function Settings({
  route,
  onRoute,
}: {
  route: Extract<AppRoute, { view: "settings" }>;
  onRoute: (r: AppRoute) => void;
}) {
  const settings = useSettings();
  const [testing, setTesting] = useState(false);
  const [result, setResult] = useState<TestResult | null>(null);

  const runTest = () => {
    setTesting(true);
    setResult(null);
    Promise.allSettled([health(), reconHealth()])
      .then(([api, recon]) => {
        setResult({
          api: api.status === "fulfilled" ? api.value : false,
          recon: recon.status === "fulfilled" ? recon.value : false,
          error:
            api.status === "rejected"
              ? errText(api.reason)
              : recon.status === "rejected"
                ? errText(recon.reason)
                : null,
        });
      })
      .finally(() => setTesting(false));
  };

  return (
    <div className="flex h-full flex-col">
      <PlaneHeader
        plane="system"
        planeLabel="System"
        route="localStorage"
        title="Settings"
        summary="Connect the console to swift-api and the recon read-model."
      />

      <div className="min-h-0 flex-1 overflow-auto p-5 space-y-4">
        <NodeErrorBanner />

        <Segmented options={TABS} value={route.tab} onChange={(tab) => onRoute({ view: "settings", tab })} />

        {route.tab === "connection" ? (
          <div className="space-y-4">
            <Card title="swift-api" desc="Jobs, manifests, metrics.">
              <div className="space-y-3">
                <Field label="API base">
                  <input
                    className={inputCls}
                    value={settings.connection.apiBase}
                    onChange={(e) => patchConnection({ apiBase: e.target.value })}
                    placeholder="/api"
                  />
                  <p className="mt-1 text-[10.5px] text-muted-foreground">
                    In dev, the Vite proxy forwards <code className="font-mono">/api</code> to the swift-api
                    server. Point this elsewhere to bypass the proxy.
                  </p>
                </Field>
                <Field label="Bearer token">
                  <input
                    type="password"
                    className={inputCls}
                    value={settings.connection.bearerToken}
                    onChange={(e) => patchConnection({ bearerToken: e.target.value })}
                    placeholder="empty = unsecured"
                    autoComplete="off"
                  />
                  <p className="mt-1 text-[10.5px] text-muted-foreground">
                    Sent as <code className="font-mono">Authorization: Bearer …</code> when non-empty. Required
                    only if swift-api is running with auth enabled.
                  </p>
                </Field>
              </div>
            </Card>

            <Card title="Recon read-model" desc="Cash statements + positions (DuckDB-over-Parquet).">
              <div className="space-y-3">
                <Field label="Recon base">
                  <input
                    className={inputCls}
                    value={settings.connection.reconBase}
                    onChange={(e) => patchConnection({ reconBase: e.target.value })}
                    placeholder="/recon"
                  />
                  <p className="mt-1 text-[10.5px] text-muted-foreground">
                    In dev, the Vite proxy forwards <code className="font-mono">/recon</code> to{" "}
                    <code className="font-mono">ingest serve</code>.
                  </p>
                </Field>
                <Field label="Static snapshot fallback">
                  <input
                    className={inputCls}
                    value={settings.connection.reconFallback}
                    onChange={(e) => patchConnection({ reconFallback: e.target.value })}
                    placeholder="/recon-snapshot.json"
                  />
                  <p className="mt-1 text-[10.5px] text-muted-foreground">
                    Served when the recon server is unreachable, so books views can still render last-known data.
                  </p>
                </Field>
              </div>
            </Card>

            <Card title="Test connection">
              <div className="flex flex-wrap items-center gap-3">
                <Btn variant="primary" onClick={runTest} disabled={testing} className="inline-flex items-center gap-1.5">
                  <PlugZap className="h-3.5 w-3.5" /> {testing ? "Testing…" : "Test connection"}
                </Btn>
                {result && (
                  <>
                    <StatusBadge variant={result.api ? "ok" : "error"}>
                      swift-api {result.api ? "up" : "down"}
                    </StatusBadge>
                    <StatusBadge variant={result.recon ? "ok" : "error"}>
                      recon {result.recon ? "up" : "down"}
                    </StatusBadge>
                  </>
                )}
              </div>
              {result?.error && <p className="mt-2 text-[11px] text-destructive">{result.error}</p>}
            </Card>
          </div>
        ) : (
          <Card title="Theme">
            <div className="flex items-center gap-3">
              <Segmented
                options={[
                  { id: "light" as const, label: "Light" },
                  { id: "dark" as const, label: "Dark" },
                ]}
                value={settings.theme}
                onChange={(t) => setTheme(t)}
              />
            </div>
            <p className="mt-2 text-[10.5px] text-muted-foreground">
              Theme is saved to localStorage and applied before first paint, so there is no flash on reload.
            </p>
          </Card>
        )}
      </div>
    </div>
  );
}
