import { useEffect, useState } from "react";
import { Sidebar } from "@/components/Sidebar";
import { Toaster } from "@/components/Toaster";
import { CommandPalette } from "@/components/CommandPalette";
import { Overview } from "@/components/views/Overview";
import { Parser } from "@/components/views/Parser";
import { Jobs } from "@/components/views/Jobs";
import { Reconciliation } from "@/components/views/Reconciliation";
import { Positions } from "@/components/views/Positions";
import { Csdr } from "@/components/views/Csdr";
import { Sources } from "@/components/views/Sources";
import { Schemas } from "@/components/views/Schemas";
import { ActivityView } from "@/components/views/Activity";
import { Settings } from "@/components/views/Settings";
import { parseHash, routeToHash, routesEqual, type AppRoute } from "@/lib/route";

export default function App() {
  const [route, setRouteState] = useState<AppRoute>(() => parseHash());

  const setRoute = (r: AppRoute) => {
    setRouteState(r);
    const h = routeToHash(r);
    if (location.hash.replace(/^#/, "") !== h) location.hash = h;
  };

  useEffect(() => {
    const onHash = () => {
      const next = parseHash();
      setRouteState((prev) => (routesEqual(prev, next) ? prev : next));
    };
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-surface-app text-foreground">
      <Sidebar route={route} onRoute={setRoute} />
      <main className="min-w-0 flex-1 overflow-hidden">
        {route.view === "overview" && <Overview onRoute={setRoute} />}
        {route.view === "parser" && <Parser route={route} onRoute={setRoute} />}
        {route.view === "jobs" && <Jobs route={route} onRoute={setRoute} />}
        {route.view === "recon" && <Reconciliation route={route} onRoute={setRoute} />}
        {route.view === "positions" && <Positions route={route} onRoute={setRoute} />}
        {route.view === "csdr" && <Csdr route={route} onRoute={setRoute} />}
        {route.view === "sources" && <Sources onRoute={setRoute} />}
        {route.view === "schemas" && <Schemas route={route} onRoute={setRoute} />}
        {route.view === "activity" && <ActivityView />}
        {route.view === "settings" && <Settings route={route} onRoute={setRoute} />}
      </main>
      <CommandPalette onRoute={setRoute} />
      <Toaster />
    </div>
  );
}
