/** @file Deterministic restart acknowledgement, outage, recovery, and timer ownership. */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  RESTART_OUTAGE_DELAY_MS,
  RESTART_OUTAGE_DURATION_MS,
  RestartFixture,
} from "../../mock-backend/src/restart-fixture";
import { handleMockRequest } from "../../mock-backend/src/server";
import { MockBackendState } from "../../mock-backend/src/state";

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

function request(path: string): Request {
  return new Request(`http://mock.test${path}`);
}

describe("restart fixture timing", () => {
  it("does nothing while disabled", () => {
    const closeStreams = vi.fn();
    const fixture = new RestartFixture(false, closeStreams);
    fixture.acknowledge();
    vi.advanceTimersByTime(10_000);
    expect(fixture.isUnavailable).toBe(false);
    expect(closeStreams).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);
    fixture.dispose();
  });

  it("closes streams once after acknowledgement and recovers after the exact outage", () => {
    const closeStreams = vi.fn();
    const fixture = new RestartFixture(true, closeStreams);
    fixture.acknowledge();
    fixture.acknowledge();
    vi.advanceTimersByTime(RESTART_OUTAGE_DELAY_MS - 1);
    expect(fixture.isUnavailable).toBe(false);
    vi.advanceTimersByTime(1);
    expect(fixture.isUnavailable).toBe(true);
    expect(closeStreams).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(RESTART_OUTAGE_DURATION_MS - 1);
    expect(fixture.isUnavailable).toBe(true);
    vi.advanceTimersByTime(1);
    expect(fixture.isUnavailable).toBe(false);
    expect(vi.getTimerCount()).toBe(0);
    fixture.dispose();
  });

  it.each([
    0,
    RESTART_OUTAGE_DELAY_MS,
  ])("cancels timers when disposed at %sms", (delay) => {
    const fixture = new RestartFixture(true, vi.fn());
    fixture.acknowledge();
    vi.advanceTimersByTime(delay);
    fixture.dispose();
    fixture.acknowledge();
    expect(vi.getTimerCount()).toBe(0);
    vi.advanceTimersByTime(10_000);
    expect(fixture.isUnavailable).toBe(false);
  });
});

describe("restart fixture transport integration", () => {
  it("preserves disabled status and continuous API availability by default", async () => {
    const state = new MockBackendState();
    expect(state.getGatewayStatus().restart_enabled).toBe(false);
    state.sendMessage({ content: "/restart", images: [] });
    vi.advanceTimersByTime(10_000);
    expect(
      (await handleMockRequest(request("/api/gateway/status"), state)).status
    ).toBe(200);
    expect(vi.getTimerCount()).toBe(0);
    state.dispose();
  });

  it("acknowledges before closing SSE, returns 503 for API routes, then permits recovery", async () => {
    const state = new MockBackendState(undefined, { restartFixture: true });
    expect(state.getGatewayStatus().restart_enabled).toBe(true);
    const chat = await handleMockRequest(request("/api/chat/events"), state);
    const logs = await handleMockRequest(request("/api/logs/events"), state);
    const chatText = chat.text();
    const logText = logs.text();
    state.sendMessage({ content: "/restart", images: [] });
    vi.advanceTimersByTime(340);
    expect(state.isGatewayUnavailable).toBe(false);
    expect(
      (await handleMockRequest(request("/api/gateway/status"), state)).status
    ).toBe(200);
    vi.advanceTimersByTime(RESTART_OUTAGE_DELAY_MS);
    expect(await chatText).toContain(
      '"type":"tool_completed","name":"restart","success":true'
    );
    expect(await logText).toContain("event: log");
    for (const path of [
      "/api/gateway/status",
      "/api/chat/events",
      "/api/logs/events",
      "/api/jobs",
    ]) {
      expect((await handleMockRequest(request(path), state)).status).toBe(503);
    }
    // SSE heartbeat timers were torn down; only fixture recovery remains.
    expect(vi.getTimerCount()).toBe(1);
    vi.advanceTimersByTime(RESTART_OUTAGE_DURATION_MS);
    expect(
      (await handleMockRequest(request("/api/gateway/status"), state)).status
    ).toBe(200);
    const recovered = await handleMockRequest(
      request("/api/chat/events"),
      state
    );
    expect(recovered.status).toBe(200);
    await recovered.body?.cancel();
    state.dispose();
    expect(vi.getTimerCount()).toBe(0);
  });
});
