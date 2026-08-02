// Session activity timeline — everything the user did this session (parses,
// uploads, job submissions, snapshot refreshes), read from the local activity
// bus. Newest first; purely client-side, no network calls.
import { useEffect, useState } from "react";
import { PlaneHeader } from "@/components/PlaneHeader";
import { StatusBadge, type StateVariant } from "@/components/StatusBadge";
import { PillarEmpty } from "@/components/PillarEmpty";
import { Btn } from "@/components/setup/kit";
import { getActivity, subscribeActivity, clearActivity, type ActivityEntry } from "@/lib/activity";
import { relTime } from "@/lib/format";
import { ScrollText } from "lucide-react";

const TONE_DOT: Record<ActivityEntry["tone"], string> = {
  ok: "bg-ok",
  info: "bg-info",
  warn: "bg-warn",
  error: "bg-destructive",
};

const TONE_BADGE: Record<ActivityEntry["tone"], StateVariant> = {
  ok: "ok",
  info: "info",
  warn: "warn",
  error: "error",
};

export function ActivityView() {
  const [entries, setEntries] = useState<ActivityEntry[]>([...getActivity()]);

  useEffect(() => {
    const onChange = () => setEntries([...getActivity()]);
    const unsubscribe = subscribeActivity(onChange);
    return unsubscribe;
  }, []);

  return (
    <div className="flex h-full flex-col">
      <PlaneHeader
        plane="system"
        planeLabel="System"
        route="session"
        title="Activity"
        summary="Everything you did this session — parses, uploads, job submissions, snapshot refreshes."
        actions={
          <Btn variant="ghost" onClick={() => clearActivity()}>
            Clear
          </Btn>
        }
      />

      <div className="min-h-0 flex-1 overflow-auto p-5 space-y-4">
        {entries.length ? (
          <ul className="divide-y divide-outline-subtle rounded-lg border border-outline-subtle bg-surface-panel">
            {entries.map((e) => (
              <li key={e.id} className="flex items-center gap-3 px-3 py-2">
                <span className={`h-2 w-2 shrink-0 rounded-full ${TONE_DOT[e.tone]}`} aria-hidden="true" />
                <StatusBadge variant={TONE_BADGE[e.tone]}>{e.kind}</StatusBadge>
                <span className="min-w-0 flex-1 truncate font-mono text-[11.5px] text-muted-foreground">
                  {e.detail}
                </span>
                <span className="shrink-0 text-right text-[11px] text-muted-foreground">{relTime(e.at)}</span>
              </li>
            ))}
          </ul>
        ) : (
          <PillarEmpty
            icon={ScrollText}
            title="No activity yet"
            body="Parse a message or submit a job and it shows up here."
          />
        )}
      </div>
    </div>
  );
}
