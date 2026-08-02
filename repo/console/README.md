# swiftpipe console

A web console for the swiftpipe SWIFT / financial-message ingestion pipeline.
The UX architecture is repurposed from the ArrowRef query-cache console (React 18
+ Vite + Tailwind, shadcn-style tokens, hash router, `useSyncExternalStore`
stores, a shared primitive kit) with the CubeCanvas grid dropped and the domain
swapped for swiftpipe. The centerpiece is the **SWIFT parser workbench**.

## Views

| Plane | View | Backend | What it shows |
|-------|------|---------|----------------|
| Pipeline | Overview | swift-api + recon | Health, throughput, recon at a glance |
| Pipeline | **Parser** | client + swift-api | Dissect a FIN message: blocks 1–5, `:NN:` tags, sequences, schema match, reconciliation read, server validation |
| Pipeline | Jobs | swift-api | Ingest jobs, manifests, per-message parse results, artifacts |
| Books | Reconciliation | recon read-model | Cash statements, Tier-1 P&L, the reconciliation equation, breaks |
| Books | Positions | recon read-model | Holdings by safekeeping account |
| Control | Sources | recon + ingest-core | Provenance and message types seen |
| Control | Schemas | build-time assets | The MT schema catalog (read-only) |
| System | Activity | local bus | This session's operations |
| System | Settings | localStorage | Connect to swift-api / recon, theme |

Plus a ⌘K command palette.

## Data sources

- **swift-api** (jobs / upload / manifest / metrics) — proxied at `/api` (default `http://127.0.0.1:8080`).
- **recon read-model** (`ingest serve`, `GET /recon/snapshot`) — proxied at `/recon` (default `http://127.0.0.1:7390`). Falls back to a static `recon-snapshot.json` when the service is down.

Override targets with `SWIFT_API_URL` / `SWIFT_RECON_URL`. Set a bearer token in
Settings → Connection if swift-api runs with auth.

## The parser

The `.fin` samples in `../examples` and the schema YAMLs in `../examples/schemas`
are compiled to `src/generated/{samples,schemas}.json` at build time by
`scripts/gen-assets.mjs`. The parser tokenizes a FIN message entirely in the
browser (`src/lib/fin.ts`) for instant structure + source spans (hover-sync raw
↔ tree), overlays the schema match + a reconciliation read, and can submit to
swift-api for the authoritative swift-core / swift-schema parse.

The client tokenizer is a **structural** splitter, not the authoritative
validator — swift-core / swift-schema on the backend remain the source of truth.

## Develop

```bash
npm install
npm run dev      # http://localhost:1460  (runs gen-assets first)
npm run build    # gen + tsc --noEmit + vite build
```
