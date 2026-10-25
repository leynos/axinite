//! Failure diagnostics use fixed labels, count every occurrence, and log once.

import { expect, it, vi } from "vitest";
import {
  boundaryFailureCount,
  recordBoundaryFailure,
} from "@/lib/api/boundary-diagnostics";

it("counts repeated failures without repeating the warning or exposing payloads", () => {
  const warnings: unknown[][] = [];
  const warning = vi.spyOn(console, "warn").mockImplementation((...values) => {
    warnings.push(values);
  });
  try {
    recordBoundaryFailure("tee-report", "invalid-json");
    recordBoundaryFailure("tee-report", "invalid-json");
    expect(boundaryFailureCount("tee-report", "invalid-json")).toBe(2);
    expect(warnings).toEqual([
      [
        "[gateway-boundary] response failure",
        { operation: "tee-report", reason: "invalid-json" },
      ],
    ]);
  } finally {
    warning.mockRestore();
  }
});
