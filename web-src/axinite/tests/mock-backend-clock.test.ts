/** @file Clock injection makes preview mutations and operational telemetry repeatable. */
import { describe, expect, it } from "vitest";
import { MockBackendState } from "../../mock-backend/src/state";

const SEED_TIME = Date.parse("2026-03-26T11:30:00.000Z");

describe("mock backend clock", () => {
  it("uses the injected clock for uptime, memory, jobs and routine activity", () => {
    let time = SEED_TIME;
    const state = new MockBackendState(() => time);
    expect(state.getGatewayStatus().uptime_secs).toBe(0);
    time += 5999;
    expect(state.getGatewayStatus().uptime_secs).toBe(5);
    const timestamp = new Date(time).toISOString();
    state.writeMemory({ path: "clock-test.md", content: "clock test" });
    expect(state.readMemory("clock-test.md").updated_at).toBe(timestamp);
    const job = state.listJobs().jobs[0];
    expect(job).toBeDefined();
    if (!job) throw new Error("Missing seeded job");
    state.restartJob(job.id);
    expect(state.getJob(job.id).started_at).toBe(timestamp);
    const routine = state.listRoutines().routines[0];
    expect(routine).toBeDefined();
    if (!routine) throw new Error("Missing seeded routine");
    time += 86_400_000;
    expect(state.summarizeRoutines().runs_today).toBe(0);
    state.triggerRoutine(routine.id);
    expect(state.getRoutine(routine.id).last_run_at).toBe(
      new Date(time).toISOString()
    );
    expect(state.summarizeRoutines().runs_today).toBe(1);
  });

  it("defaults to the same fixture timeline on every stub run", () => {
    const left = new MockBackendState();
    const right = new MockBackendState();
    expect(left.createThread()).toEqual(right.createThread());
    expect(left.getGatewayStatus().uptime_secs).toBe(0);
  });
});
