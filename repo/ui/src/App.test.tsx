import { render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";

describe("App", () => {
  beforeEach(() => {
    const storage = new Map<string, string>();
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => storage.set(key, value),
      removeItem: (key: string) => storage.delete(key),
      clear: () => storage.clear(),
    });
    vi.stubGlobal(
      "fetch",
      vi.fn((input: RequestInfo | URL) => {
        const url = String(input);
        if (url.endsWith("/healthz")) {
          return Promise.resolve(new Response("ok", { status: 200 }));
        }
        if (url.endsWith("/metrics/json")) {
          return Promise.resolve(
            Response.json({
              jobs_submitted: 0,
              jobs_completed: 0,
              jobs_failed: 0,
              jobs_in_flight: 0,
              queue_depth: 0,
            })
          );
        }
        if (url.startsWith("/v1/jobs")) {
          return Promise.resolve(Response.json([]));
        }
        return Promise.reject(new Error(`Unexpected fetch: ${url}`));
      })
    );
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("renders the dashboard shell without throwing", async () => {
    render(<App />);

    expect(screen.getByText("SwiftPipe")).toBeInTheDocument();
    expect(screen.getByText("Recent Jobs")).toBeInTheDocument();
    expect(
      screen.getByText("Select a job or submit one to see details")
    ).toBeInTheDocument();
    expect(await screen.findByText("Submitted")).toBeInTheDocument();
  });
});
