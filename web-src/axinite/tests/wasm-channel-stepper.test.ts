import { describe, expect, it } from "vitest";

import { computeStepperModel } from "@/components/wasm-channel-stepper";

describe("computeStepperModel", () => {
  it("marks only the first step complete when freshly installed", () => {
    const model = computeStepperModel("installed");
    expect(model.reachedIdx).toBe(0);
    expect(model.states).toEqual(["completed", "pending", "pending"]);
    expect(model.awaitingPairing).toBe(false);
  });

  it("defaults an unset status to installed", () => {
    expect(computeStepperModel(undefined).states).toEqual([
      "completed",
      "pending",
      "pending",
    ]);
  });

  it("reaches the second step when configured", () => {
    const model = computeStepperModel("configured");
    expect(model.reachedIdx).toBe(1);
    expect(model.states).toEqual(["completed", "completed", "pending"]);
  });

  it("shows the final step in progress while awaiting pairing", () => {
    const model = computeStepperModel("pairing");
    expect(model.reachedIdx).toBe(2);
    expect(model.states).toEqual(["completed", "completed", "in-progress"]);
    expect(model.awaitingPairing).toBe(true);
  });

  it("completes every step when active", () => {
    expect(computeStepperModel("active").states).toEqual([
      "completed",
      "completed",
      "completed",
    ]);
  });

  it("marks the reached step failed when activation fails", () => {
    const model = computeStepperModel("failed");
    expect(model.reachedIdx).toBe(2);
    expect(model.states).toEqual(["completed", "completed", "failed"]);
  });
});
