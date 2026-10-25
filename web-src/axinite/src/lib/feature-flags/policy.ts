/** @file Framework-independent feature-flag precedence policy. */

import {
  FEATURE_FLAGS,
  type FeatureFlagName,
  getFeatureFlagDefaults,
} from "./registry";

/** Browser-local choices; omitted flags follow the gateway or compiled default. */
export type OverrideMap = Partial<Record<FeatureFlagName, boolean>>;

/**
 * Resolve every flag: a local override wins over a server value, which wins
 * over the registry default. Unknown keys are ignored.
 */
export function resolveFeatureFlags(
  serverFlags: Record<string, boolean>,
  overrides: OverrideMap
): Record<FeatureFlagName, boolean> {
  const defaults = getFeatureFlagDefaults();

  for (const flag of FEATURE_FLAGS) {
    const serverValue = serverFlags[flag.name];
    const overrideValue = overrides[flag.name];

    if (overrideValue !== undefined) {
      defaults[flag.name] = overrideValue;
      continue;
    }

    if (serverValue !== undefined) {
      defaults[flag.name] = serverValue;
    }
  }

  return defaults;
}
