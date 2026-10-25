/**
 * @file Gateway-level API wrapper: `/api/gateway/status` for the shell's status
 * pill and restart polling, and `/api/features` for runtime feature flags.
 * Status polling falls back when unreachable; flag requests remain fallible.
 */

import { recordBoundaryFailure } from "@/lib/api/boundary-diagnostics";
import { requestJson } from "@/lib/api/client";
import type {
  FeatureFlagsResponse,
  GatewayStatus,
  GatewayStatusResponse,
} from "@/lib/api/contracts";
import { isBoolean, isJsonObject, type JsonValue } from "@/lib/api/json-value";

/** Fetch the full gateway status payload, or `null` when unreachable. */
export async function fetchGatewayStatusRaw(): Promise<GatewayStatusResponse | null> {
  try {
    return await requestJson<GatewayStatusResponse>("/api/gateway/status");
  } catch {
    return null;
  }
}

/** Derive the topbar status pill from a raw status payload. */
export function deriveGatewayStatus(
  payload: GatewayStatusResponse | null
): GatewayStatus {
  if (!payload) {
    return {
      label: "Preview",
      detail: "Mock gateway unavailable",
    };
  }

  const connections = payload.total_connections;
  return {
    label: connections > 0 ? "Connected" : "Preview",
    detail: `v${payload.version} · ${connections} live browser stream${connections === 1 ? "" : "s"}`,
  };
}

/** GET /api/gateway/status: fetch the topbar status pill, with a fallback. */
export async function fetchGatewayStatus(): Promise<GatewayStatus> {
  return deriveGatewayStatus(await fetchGatewayStatusRaw());
}

/** GET /api/features: fetch runtime feature flags; transport errors propagate. */
export async function fetchRuntimeFeatureFlags(): Promise<
  Record<string, boolean>
> {
  const payload = await requestJson<JsonValue>("/api/features");
  const root = isJsonObject(payload) ? payload : null;
  const source = root && isJsonObject(root["flags"]) ? root["flags"] : root;
  if (source === null) {
    recordBoundaryFailure("feature-flags", "invalid-payload");
  }
  const flags: FeatureFlagsResponse = {};

  for (const [name, value] of Object.entries(source ?? {})) {
    if (isBoolean(value)) {
      flags[name] = value;
    } else {
      recordBoundaryFailure("feature-flags", "invalid-payload");
    }
  }

  return flags;
}
