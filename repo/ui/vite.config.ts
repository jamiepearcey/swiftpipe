/// <reference types="vitest" />

import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  base: "/app/",
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
  test: {
    environment: "jsdom",
    setupFiles: "./src/test/setup.ts",
  },
  server: {
    proxy: {
      "/v1": "http://localhost:8080",
      "/metrics": "http://localhost:8080",
      "/healthz": "http://localhost:8080",
      "/openapi.json": "http://localhost:8080",
    },
  },
});
