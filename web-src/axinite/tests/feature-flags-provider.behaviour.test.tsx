/** @file Runtime flag failures retain defaults and emit a safe operational signal. */

import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { getFeatureFlagDefaults } from "@/lib/feature-flags/registry";
import {
  FeatureFlagProvider,
  useFeatureFlags,
} from "@/lib/feature-flags/runtime";
import { GatewayTestServer } from "./support/gateway-test-server";
import { setApiRequestHandler } from "./support/i18n-test-runtime";

let gateway: GatewayTestServer;

beforeEach(() => {
  gateway = new GatewayTestServer().respond("GET", "/api/features", {});
});

afterEach(() => {
  localStorage.clear();
  vi.restoreAllMocks();
  vi.unstubAllEnvs();
  setApiRequestHandler(undefined);
});

function FlagsProbe() {
  const flags = useFeatureFlags();
  return (
    <>
      <output data-testid="flags">
        {JSON.stringify(flags.resolvedFlags())}
      </output>
      <button
        type="button"
        onClick={() => flags.setOverride("route_jobs", false)}
      >
        Hide jobs
      </button>
    </>
  );
}

describe("runtime feature-flag failure", () => {
  it("reports a sanitized failure in production and keeps defaults plus local overrides", async () => {
    vi.stubEnv("DEV", false);
    gateway.fail(
      "GET",
      "/api/features",
      500,
      "secret-token and deployment-internal details"
    );
    const report = vi.spyOn(console, "error").mockImplementation(() => {});
    localStorage.setItem(
      "axinite.feature-flag-overrides",
      JSON.stringify({ route_jobs: false })
    );
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(() => (
      <QueryClientProvider client={client}>
        <FeatureFlagProvider>
          <FlagsProbe />
        </FeatureFlagProvider>
      </QueryClientProvider>
    ));
    await waitFor(() =>
      expect(report).toHaveBeenCalledWith(
        "[feature-flags] Failed to load runtime flags; using local overrides and defaults."
      )
    );
    expect(JSON.parse(screen.getByTestId("flags").textContent)).toEqual({
      ...getFeatureFlagDefaults(),
      route_jobs: false,
    });
    expect(JSON.stringify(report.mock.calls)).not.toContain("secret-token");
  });

  it("reports malformed local overrides without exposing their contents", () => {
    localStorage.setItem(
      "axinite.feature-flag-overrides",
      "secret invalid JSON"
    );
    const report = vi.spyOn(console, "error").mockImplementation(() => {});
    const client = new QueryClient();
    render(() => (
      <QueryClientProvider client={client}>
        <FeatureFlagProvider>
          <FlagsProbe />
        </FeatureFlagProvider>
      </QueryClientProvider>
    ));
    expect(report).toHaveBeenCalledWith(
      "[feature-flags] Failed to hydrate local overrides; using defaults."
    );
    expect(JSON.parse(screen.getByTestId("flags").textContent)).toEqual(
      getFeatureFlagDefaults()
    );
  });

  it("retains the in-page override when persistence is unavailable", () => {
    // happy-dom binds storage methods to each instance after first access.
    vi.spyOn(globalThis.localStorage, "setItem").mockImplementation(() => {
      throw new Error("Storage unavailable");
    });
    const report = vi.spyOn(console, "error").mockImplementation(() => {});
    const client = new QueryClient();
    render(() => (
      <QueryClientProvider client={client}>
        <FeatureFlagProvider>
          <FlagsProbe />
        </FeatureFlagProvider>
      </QueryClientProvider>
    ));
    fireEvent.click(screen.getByRole("button", { name: "Hide jobs" }));
    expect(JSON.parse(screen.getByTestId("flags").textContent).route_jobs).toBe(
      false
    );
    expect(report).toHaveBeenCalledWith(
      "[feature-flags] Failed to persist local overrides; changes apply only to this page."
    );
  });
});
