// TEE (trusted execution environment) attestation client.
//
// Attestation is served by a sibling host derived from the browser location
// (`https://api.<domain>`), mirroring the legacy contract in `app.js`. It is
// inert on localhost, loopback, bare IP, and single-label hosts, where no such
// sibling host exists.

export type TeeStatus = {
  image_digest: string;
  [key: string]: unknown;
};

export type TeeReport = {
  tls_certificate_fingerprint?: string;
  report_data?: string;
  vm_config?: string;
  [key: string]: unknown;
};

const IPV4_PATTERN =
  /^(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)$/u;

function currentHostname(): string {
  if (typeof window === "undefined") {
    return "";
  }
  return window.location.hostname;
}

function currentProtocol(): string {
  if (typeof window === "undefined") {
    return "https:";
  }
  return window.location.protocol;
}

/**
 * Derive the attestation host base URL from the browser hostname.
 *
 * Returns `null` for localhost, loopback, bare IPv4/IPv6 addresses, and
 * single-label hosts, matching the legacy `teeApiBase` behaviour.
 */
export function teeApiBase(
  hostname: string = currentHostname()
): string | null {
  if (!hostname) {
    return null;
  }

  if (
    hostname === "localhost" ||
    hostname === "127.0.0.1" ||
    hostname === "::1" ||
    IPV4_PATTERN.test(hostname) ||
    hostname.includes(":")
  ) {
    return null;
  }

  const parts = hostname.split(".");
  if (parts.length < 2) {
    return null;
  }

  const domain = parts.slice(1).join(".");
  return `${currentProtocol()}//api.${domain}`;
}

/** The instance name is the first label of the hostname. */
export function teeInstanceName(hostname: string = currentHostname()): string {
  return hostname.split(".")[0];
}

async function fetchJson<T>(url: string): Promise<T> {
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`TEE request failed for ${url} with ${response.status}`);
  }
  return (await response.json()) as T;
}

/**
 * Fetch attestation status for the current instance. Resolves to `null` when
 * the host has no attestation surface (see {@link teeApiBase}).
 */
export async function checkTeeStatus(): Promise<TeeStatus | null> {
  const base = teeApiBase();
  if (!base) {
    return null;
  }

  const name = teeInstanceName();
  const status = await fetchJson<TeeStatus>(
    `${base}/instances/${encodeURIComponent(name)}/attestation`
  );

  if (typeof status.image_digest !== "string") {
    throw new Error("TEE attestation response is missing image_digest");
  }

  return status;
}

let reportCache: TeeReport | null = null;

/** Fetch the attestation report, caching the first successful response. */
export async function fetchTeeReport(): Promise<TeeReport> {
  if (reportCache) {
    return reportCache;
  }

  const base = teeApiBase();
  if (!base) {
    throw new Error("TEE attestation is unavailable on this host");
  }

  const report = await fetchJson<TeeReport>(`${base}/attestation/report`);
  reportCache = report;
  return report;
}

/** Testing helper: clear the memoized report between cases. */
export function resetTeeReportCache(): void {
  reportCache = null;
}
