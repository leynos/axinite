/** @file Flag requests expose transport failure separately from valid empty flags. */
import { afterEach, describe, expect, it } from "vitest";
import { fetchRuntimeFeatureFlags } from "@/lib/api/gateway";
import { GatewayTestServer } from "./support/gateway-test-server";
import { setApiRequestHandler } from "./support/i18n-test-runtime";

afterEach(() => setApiRequestHandler(undefined));

describe("runtime flag transport", () => {
  it("propagates request failure to the provider", async () => {
    new GatewayTestServer().fail(
      "GET",
      "/api/features",
      503,
      "gateway unavailable"
    );
    await expect(fetchRuntimeFeatureFlags()).rejects.toThrow(
      "gateway unavailable"
    );
  });

  it.each([
    { route_jobs: false },
    { flags: { route_jobs: false } },
  ])("accepts flat and wrapped payloads while retaining boolean values", async (payload) => {
    new GatewayTestServer().respond("GET", "/api/features", payload);
    await expect(fetchRuntimeFeatureFlags()).resolves.toEqual({
      route_jobs: false,
    });
  });

  it("accepts a successful empty response", async () => {
    new GatewayTestServer().respond("GET", "/api/features", {});
    await expect(fetchRuntimeFeatureFlags()).resolves.toEqual({});
  });
});
