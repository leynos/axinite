import { describe, expect, it } from "vitest";

import { isStreamingApiPath } from "../../mock-backend/src/streaming-routes";

describe("mock backend streaming routes", () => {
  it("marks the long-lived SSE endpoints as streaming routes", () => {
    expect(isStreamingApiPath("/api/chat/events")).toBe(true);
    expect(isStreamingApiPath("/api/logs/events")).toBe(true);
  });

  it("does not mark regular JSON routes as streaming routes", () => {
    expect(isStreamingApiPath("/api/chat/send")).toBe(false);
    expect(isStreamingApiPath("/api/logs/level")).toBe(false);
  });
});
