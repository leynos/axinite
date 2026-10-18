/** @file Runtime flag failures retain defaults and emit a safe operational signal. */

import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import { afterEach, describe, expect, it, vi } from "vitest";

import { fetchRuntimeFeatureFlags } from "@/lib/api/gateway";
import { getFeatureFlagDefaults } from "@/lib/feature-flags/registry";
import {
  FeatureFlagProvider,
  useFeatureFlags,
} from "@/lib/feature-flags/runtime";

vi.mock("@/lib/api/gateway", () => ({ fetchRuntimeFeatureFlags: vi.fn() }));

afterEach(() => {
  localStorage.clear();
  vi.restoreAllMocks();
  vi.unstubAllEnvs();
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
    const error = new Error("secret-token and deployment-internal details");
    vi.mocked(fetchRuntimeFeatureFlags).mockRejectedValue(error);
    const report = vi
      .spyOn(console, "error")
      .mockImplementation(() => undefined);
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
    expect(JSON.parse(screen.getByTestId("flags").textContent ?? "{}")).toEqual(
      {
        ...getFeatureFlagDefaults(),
        route_jobs: false,
      }
    );
    expect(JSON.stringify(report.mock.calls)).not.toContain("secret-token");
  });

  it("reports malformed local overrides without exposing their contents", () => {
    localStorage.setItem(
      "axinite.feature-flag-overrides",
      "secret invalid JSON"
    );
    vi.mocked(fetchRuntimeFeatureFlags).mockResolvedValue({});
    const report = vi
      .spyOn(console, "error")
      .mockImplementation(() => undefined);
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
    expect(JSON.parse(screen.getByTestId("flags").textContent ?? "{}")).toEqual(
      getFeatureFlagDefaults()
    );
  });

  it("retains the in-page override when persistence is unavailable", () => {
    vi.mocked(fetchRuntimeFeatureFlags).mockResolvedValue({});
    // happy-dom binds storage methods to each instance after first access.
    vi.spyOn(window.localStorage, "setItem").mockImplementation(() => {
      throw new Error("Storage unavailable");
    });
    const report = vi
      .spyOn(console, "error")
      .mockImplementation(() => undefined);
    const client = new QueryClient();
    render(() => (
      <QueryClientProvider client={client}>
        <FeatureFlagProvider>
          <FlagsProbe />
        </FeatureFlagProvider>
      </QueryClientProvider>
    ));
    fireEvent.click(screen.getByRole("button", { name: "Hide jobs" }));
    expect(
      JSON.parse(screen.getByTestId("flags").textContent ?? "{}").route_jobs
    ).toBe(false);
    expect(report).toHaveBeenCalledWith(
      "[feature-flags] Failed to persist local overrides; changes apply only to this page."
    );
  });
});
