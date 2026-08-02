// Books plane — holdings/positions over the recon read-model (GET /recon/snapshot).
import { Boxes, Hash, Landmark, RefreshCw, ServerCrash, PackageOpen, AlertTriangle } from "lucide-react";
import { PlaneHeader } from "@/components/PlaneHeader";
import { StatTile } from "@/components/StatTile";
import { Select } from "@/components/Select";
import { DataTable, type Column } from "@/components/DataTable";
import { TagChip } from "@/components/TagChip";
import { NodeErrorBanner } from "@/components/NodeErrorBanner";
import { PillarEmpty } from "@/components/PillarEmpty";
import { Btn } from "@/components/setup/kit";
import { useSnapshot } from "@/lib/useSnapshot";
import { positionsByAccount } from "@/lib/recon";
import { fmtAmount, fmtInt, relTime } from "@/lib/format";
import type { AppRoute } from "@/lib/route";
import type { SnapshotPosition } from "@/lib/types";

/** "MT535" -> "535"; null when the message type doesn't look like a SWIFT MT tag. */
function mtTag(mt: string): string | null {
  const m = mt.match(/^MT(\d{3})$/i);
  return m ? m[1] : null;
}

function fmtQty(n: number): string {
  return Number.isInteger(n) ? fmtInt(n) : fmtAmount(n);
}

const COLUMNS: Column<SnapshotPosition>[] = [
  {
    key: "isin",
    header: "ISIN",
    mono: true,
    sortValue: (p) => p.isin,
    render: (p) => p.isin,
  },
  {
    key: "desc",
    header: "Instrument",
    sortValue: (p) => p.desc ?? "",
    render: (p) => p.desc || <span className="text-muted-foreground">—</span>,
  },
  {
    key: "quantity",
    header: "Quantity",
    align: "right",
    mono: true,
    sortValue: (p) => p.quantity,
    render: (p) => fmtQty(p.quantity),
  },
  {
    key: "messageType",
    header: "Message",
    sortValue: (p) => p.messageType,
    render: (p) => {
      const tag = mtTag(p.messageType);
      return tag ? <TagChip tag={tag} /> : <span className="font-mono text-[11.5px]">{p.messageType}</span>;
    },
  },
  {
    key: "source",
    header: "Source",
    sortValue: (p) => p.source,
    render: (p) => p.source,
  },
];

export function Positions({
  route,
  onRoute,
}: {
  route: Extract<AppRoute, { view: "positions" }>;
  onRoute: (r: AppRoute) => void;
}) {
  const { result, error, at, refresh } = useSnapshot();
  const snapshot = result?.snapshot;

  const refreshBtn = (
    <Btn variant="ghost" onClick={refresh} className="inline-flex items-center gap-1.5">
      <RefreshCw className="h-3.5 w-3.5" /> Refresh
    </Btn>
  );

  if (error && !result) {
    return (
      <div className="flex h-full flex-col">
        <PlaneHeader
          plane="books"
          planeLabel="Books"
          route="GET /recon/snapshot"
          title="Positions"
          summary="Custody holdings by safekeeping account."
          actions={refreshBtn}
        />
        <div className="min-h-0 flex-1 overflow-auto p-5 space-y-4">
          <NodeErrorBanner error={error} />
          <PillarEmpty
            icon={ServerCrash}
            title="Unable to load positions"
            body="The recon read-model didn't respond. Check the connection settings and try again."
          />
        </div>
      </div>
    );
  }

  const allPositions = snapshot?.positions ?? [];
  const groups = snapshot ? positionsByAccount(snapshot) : [];
  const visibleGroups = route.account ? groups.filter((g) => g.account === route.account) : groups;

  const distinctIsins = new Set(allPositions.map((p) => p.isin)).size;

  const accountOptions = [
    { value: "", label: "All accounts" },
    ...groups.map((g) => ({ value: g.account, label: g.account, meta: String(g.positions.length) })),
  ];

  return (
    <div className="flex h-full flex-col">
      <PlaneHeader
        plane="books"
        planeLabel="Books"
        route="GET /recon/snapshot"
        title="Positions"
        summary="Custody holdings by safekeeping account."
        actions={refreshBtn}
      />
      <div className="min-h-0 flex-1 overflow-auto p-5 space-y-4">
        <NodeErrorBanner error={error} />

        {result?.stale && (
          <div className="flex items-center gap-2 rounded-lg border border-warn/45 bg-warn/10 px-3 py-2 text-[12px] text-warn">
            <AlertTriangle className="h-4 w-4 shrink-0" />
            Offline snapshot · as of {relTime(at)}
          </div>
        )}

        <div className="grid grid-cols-3 gap-3">
          <StatTile label="Total positions" value={fmtInt(allPositions.length)} icon={Boxes} accent="books" />
          <StatTile label="Distinct ISINs" value={fmtInt(distinctIsins)} icon={Hash} accent="books" />
          <StatTile label="Accounts" value={fmtInt(groups.length)} icon={Landmark} accent="books" />
        </div>

        {allPositions.length === 0 ? (
          <PillarEmpty
            icon={PackageOpen}
            title="No positions yet"
            body="No positions yet — ingest an MT535/MT540 holdings message."
          />
        ) : (
          <>
            <div className="max-w-xs">
              <Select
                value={route.account ?? ""}
                onChange={(v) => onRoute({ view: "positions", account: v || undefined })}
                options={accountOptions}
                aria-label="Filter by safekeeping account"
              />
            </div>

            <div className="space-y-4">
              {visibleGroups.map((g) => (
                <div key={g.account} className="space-y-1.5">
                  <div className="flex items-center gap-2 px-1">
                    <span className="font-mono text-[12.5px] font-medium text-foreground">{g.account}</span>
                    <span className="text-[11px] text-muted-foreground">
                      {fmtInt(g.positions.length)} position{g.positions.length === 1 ? "" : "s"}
                    </span>
                  </div>
                  <DataTable
                    columns={COLUMNS}
                    rows={g.positions}
                    rowKey={(p, i) => `${g.account}-${p.isin}-${i}`}
                  />
                </div>
              ))}
            </div>
          </>
        )}
      </div>
    </div>
  );
}
