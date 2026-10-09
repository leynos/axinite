/**
 * Port configuration shared by the stub runtime's processes (the mock API,
 * the preview server, and the `scripts/dev.ts` supervisor).
 */

/**
 * Read a TCP port from an environment variable, falling back to `fallback`
 * when it is unset. Rejects anything but an integer in 1–65535 with an error
 * naming the variable, so a typo fails at start-up instead of propagating
 * `NaN` into child processes.
 */
export function parsePort(
  name: string,
  raw: string | undefined,
  fallback: number
): number {
  if (raw === undefined || raw.trim() === "") {
    return fallback;
  }
  const port = Number(raw.trim());
  if (!Number.isInteger(port) || port < 1 || port > 65_535) {
    throw new Error(
      `${name} must be an integer TCP port between 1 and 65535, got "${raw}"`
    );
  }
  return port;
}
