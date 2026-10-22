/**
 * @file TEE (trusted execution environment) attestation client.
 *
 * Attestation is served by a sibling host derived from the browser location
 * (`https://api.<domain>`), mirroring the legacy contract in `app.js`. It is
 * inert on localhost, loopback, bare IP, and single-label hosts, where no such
 * sibling host exists.
 */

import { withRequestTimeout } from "@/lib/api/client";
import {
  isJsonObject,
  isJsonValue,
  isString,
  type JsonValue,
} from "@/lib/api/json-value";

export type TeeStatus = {
  image_digest: string;
  [key: string]: JsonValue;
};

export type TeeReport = {
  tls_certificate_fingerprint?: string;
  report_data?: string;
  vm_config?: string;
  [key: string]: JsonValue | undefined;
};

const IPV4_PATTERN =
  /^(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)$/u;

function currentHostname(): string {
  return globalThis.location.hostname;
}

function currentProtocol(): string {
  return globalThis.location.protocol;
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
  return hostname.split(".")[0] ?? "";
}

function fetchJson(url: string): Promise<JsonValue> {
  return withRequestTimeout(async (signal) => {
    const response = await fetch(url, { signal });
    if (!response.ok) {
      throw new Error(`TEE request failed for ${url} with ${response.status}`);
    }
    const payload: unknown = await response.json();
    if (!isJsonValue(payload)) {
      throw new TypeError("TEE response is not a valid JSON value");
    }
    return payload;
  });
}

function parseTeeStatus(payload: JsonValue) {
  if (!isJsonObject(payload)) {
    throw new TypeError("TEE attestation response must be an object");
  }

  const imageDigest = payload["image_digest"];
  if (!isString(imageDigest)) {
    throw new TypeError("TEE attestation response is missing image_digest");
  }

  const status = { ...payload, image_digest: imageDigest } satisfies TeeStatus;
  return status;
}

function parseTeeReport(payload: JsonValue) {
  if (!isJsonObject(payload)) {
    throw new TypeError("TEE report response must be an object");
  }

  const report = { ...payload } satisfies TeeReport;
  const stringFields = [
    "tls_certificate_fingerprint",
    "report_data",
    "vm_config",
  ] as const;

  for (const field of stringFields) {
    const value = payload[field];
    if (value !== undefined) {
      if (!isString(value)) {
        throw new TypeError(`TEE report field ${field} must be a string`);
      }
      report[field] = value;
    }
  }

  return report;
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
  const payload = await fetchJson(
    `${base}/instances/${encodeURIComponent(name)}/attestation`
  );
  return parseTeeStatus(payload);
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

  const payload = await fetchJson(`${base}/attestation/report`);
  const report = parseTeeReport(payload);
  reportCache = report;
  return report;
}

/** Testing helper: clear the memoized report between cases. */
export function resetTeeReportCache(): void {
  reportCache = null;
}
