import { requestJson } from "@/lib/api/client";
import type {
  FeatureFlagsResponse,
  GatewayStatus,
  GatewayStatusResponse,
} from "@/lib/api/contracts";

export async function fetchGatewayStatus(): Promise<GatewayStatus> {
  try {
    const payload = await requestJson<GatewayStatusResponse>(
      "/api/gateway/status"
    );

    return {
      label: payload.total_connections > 0 ? "Connected" : "Preview",
      detail: `v${payload.version} · ${payload.total_connections} live browser stream${payload.total_connections === 1 ? "" : "s"}`,
    };
  } catch {
    return {
      label: "Preview",
      detail: "Mock gateway unavailable",
    };
  }
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
