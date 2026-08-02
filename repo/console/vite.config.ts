import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "node:path";

// swiftpipe console. Two backend surfaces are proxied so the browser talks to
// them without CORS:
//   /api    → swift-api    (jobs / upload / manifest / metrics)   default :8080
//   /recon  → `ingest serve` recon read-model (GET /recon/snapshot)  default :7390
// Override the targets with SWIFT_API_URL / SWIFT_RECON_URL.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  resolve: {
    alias: { "@": path.resolve(__dirname, "./src") },
    dedupe: ["react", "react-dom"],
  },
  server: {
    port: 1460,
    strictPort: true,
    proxy: {
      "/api": {
        target: process.env.SWIFT_API_URL ?? "http://127.0.0.1:8080",
        changeOrigin: true,
        rewrite: (p) => p.replace(/^\/api/, ""),
      },
      "/recon": {
        target: process.env.SWIFT_RECON_URL ?? "http://127.0.0.1:7390",
        changeOrigin: true,
        // Strip the `/recon` proxy prefix so `reconBase="/recon"` + path
        // `/recon/snapshot` resolves to the server's real `/recon/snapshot`
        // (symmetric with the `/api` proxy above).
        rewrite: (p) => p.replace(/^\/recon/, ""),
      },
      // CSDR penalty read-model — same `ingest serve` process as `/recon`.
      "/csdr": {
        target: process.env.SWIFT_CSDR_URL ?? process.env.SWIFT_RECON_URL ?? "http://127.0.0.1:7390",
        changeOrigin: true,
        rewrite: (p) => p.replace(/^\/csdr/, ""),
      },
    },
  },
  build: { outDir: "dist", target: "es2022", sourcemap: false },
});
