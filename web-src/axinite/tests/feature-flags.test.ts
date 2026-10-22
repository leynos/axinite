/**
 * @file Tests for feature-flag resolution: local overrides win over server
 * values, which win over registry defaults, and unknown keys are ignored.
 */

import { describe, expect, it } from "vitest";
import { resolveFeatureFlags } from "@/lib/feature-flags/policy";
import { getFeatureFlagDefaults } from "@/lib/feature-flags/registry";

describe("feature flag resolution", () => {
  it("applies server values before defaults", () => {
    const resolved = resolveFeatureFlags({ route_jobs: false }, {});
    expect(resolved.route_jobs).toBe(false);
  });

  it("prefers local overrides over server values", () => {
    const resolved = resolveFeatureFlags(
      { route_jobs: false },
      { route_jobs: true }
    );
    expect(resolved.route_jobs).toBe(true);
  });

  it("returns the defaults when neither server flags nor overrides are present", () => {
    const resolved = resolveFeatureFlags({}, {});
    expect(resolved).toEqual(getFeatureFlagDefaults());
  });

  it("ignores unknown server and override keys", () => {
    const overrides: Parameters<typeof resolveFeatureFlags>[1] &
      Record<string, boolean> = { unknown_flag: false };

    const resolved = resolveFeatureFlags(
      { route_jobs: false, unknown_flag: true },
      overrides
    );

    expect(resolved.route_jobs).toBe(false);
    expect("unknown_flag" in resolved).toBe(false);
  });

  it("falls back to the server or default when no override is present", () => {
    const resolved = resolveFeatureFlags({ route_jobs: false }, {});

    expect(resolved.route_jobs).toBe(false);
    expect(resolved.panel_logs).toBe(getFeatureFlagDefaults().panel_logs);
  });
});
