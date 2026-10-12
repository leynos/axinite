/** @file Flag requests expose transport failure separately from valid empty flags. */
import { afterEach, describe, expect, it, vi } from "vitest";
import { requestJson } from "@/lib/api/client";
import { fetchRuntimeFeatureFlags } from "@/lib/api/gateway";

vi.mock("@/lib/api/client", () => ({ requestJson: vi.fn() }));
afterEach(() => vi.resetAllMocks());

describe("runtime flag transport", () => {
  it("propagates request failure to the provider", async () => {
    const failure = new Error("gateway unavailable");
    vi.mocked(requestJson).mockRejectedValue(failure);
    await expect(fetchRuntimeFeatureFlags()).rejects.toBe(failure);
  });

  it.each([
    { route_jobs: false },
    { flags: { route_jobs: false } },
  ])("accepts flat and wrapped payloads while retaining boolean values", async (payload) => {
    vi.mocked(requestJson).mockResolvedValue(payload);
    await expect(fetchRuntimeFeatureFlags()).resolves.toEqual({
      route_jobs: false,
    });
  });

  it("accepts a successful empty response", async () => {
    vi.mocked(requestJson).mockResolvedValue({});
    await expect(fetchRuntimeFeatureFlags()).resolves.toEqual({});
  });
});
