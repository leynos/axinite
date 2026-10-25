/** @file Flag requests expose transport failure separately from valid empty flags. */
import { afterEach, describe, expect, it } from "vitest";
import { boundaryFailureCount } from "@/lib/api/boundary-diagnostics";
import { fetchRuntimeFeatureFlags } from "@/lib/api/gateway";
import { GatewayTestServer } from "./support/gateway-test-server";
import { setApiRequestHandler } from "./support/i18n-test-runtime";

afterEach(() => setApiRequestHandler(undefined));

describe("runtime flag transport", () => {
  it.each([
    null,
    [],
    { flags: null },
    { flags: { route_jobs: "false" } },
    { route_jobs: 0 },
  ])("records malformed flag payloads while retaining the empty fallback", async (payload) => {
    new GatewayTestServer().respond({
      method: "GET",
      path: "/api/features",
      payload,
    });
    const failures = boundaryFailureCount("feature-flags", "invalid-payload");
    await expect(fetchRuntimeFeatureFlags()).resolves.toEqual({});
    expect(boundaryFailureCount("feature-flags", "invalid-payload")).toBe(
      failures + 1
    );
  });
  it("propagates request failure to the provider", async () => {
    new GatewayTestServer().fail({
      method: "GET",
      path: "/api/features",
      status: 503,
      message: "gateway unavailable",
    });
    await expect(fetchRuntimeFeatureFlags()).rejects.toThrow(
      "gateway unavailable"
    );
  });

  it.each([
    { route_jobs: false },
    { flags: { route_jobs: false } },
  ])("accepts flat and wrapped payloads while retaining boolean values", async (payload) => {
    new GatewayTestServer().respond({
      method: "GET",
      path: "/api/features",
      payload: payload,
    });
    await expect(fetchRuntimeFeatureFlags()).resolves.toEqual({
      route_jobs: false,
    });
  });

  it("accepts a successful empty response", async () => {
    new GatewayTestServer().respond({
      method: "GET",
      path: "/api/features",
      payload: {},
    });
    await expect(fetchRuntimeFeatureFlags()).resolves.toEqual({});
  });
});
