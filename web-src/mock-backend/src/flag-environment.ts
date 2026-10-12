/** @file Environment adapter for the mock backend's injected flag overlay. */

/**
 * Parse configured FEATURE_FLAG_ values once at composition. Only the literal
 * case-insensitive true enables a flag; unset values supply no override.
 * Resolution ignores names outside the compiled registry.
 */
export function parseMockFeatureFlagEnvironment(
  environment: Readonly<Record<string, string | undefined>>
): Record<string, boolean> {
  return Object.fromEntries(
    Object.entries(environment)
      .filter(
        ([key, value]) =>
          key.startsWith("FEATURE_FLAG_") && typeof value === "string"
      )
      .map(([key, value]) => [
        key.slice("FEATURE_FLAG_".length).toLowerCase(),
        value?.toLowerCase() === "true",
      ])
  );
}
