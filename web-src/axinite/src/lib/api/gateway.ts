import { requestJson } from "@/lib/api/client";
import type {
  FeatureFlagsResponse,
  GatewayStatus,
  GatewayStatusResponse,
} from "@/lib/api/contracts";

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

  const connections = payload.total_connections ?? 0;
  return {
    label: connections > 0 ? "Connected" : "Preview",
    detail: `v${payload.version} · ${connections} live browser stream${connections === 1 ? "" : "s"}`,
  };
}

export async function fetchGatewayStatus(): Promise<GatewayStatus> {
  return deriveGatewayStatus(await fetchGatewayStatusRaw());
}

export async function fetchRuntimeFeatureFlags(): Promise<
  Record<string, boolean>
> {
  try {
    const payload = await requestJson<
      FeatureFlagsResponse | Record<string, boolean>
    >("/api/features");

    const source =
      typeof payload === "object" &&
      payload !== null &&
      "flags" in payload &&
      typeof payload.flags === "object" &&
      payload.flags !== null
        ? payload.flags
        : payload;

    return Object.entries(source).reduce<Record<string, boolean>>(
      (flags, [name, value]) => {
        if (typeof value === "boolean") {
          flags[name] = value;
        }
        return flags;
      },
      {}
    );
  } catch {
    return {};
  }
}
