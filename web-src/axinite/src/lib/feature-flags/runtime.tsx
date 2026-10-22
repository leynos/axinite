/**
 * @file Feature-flag context provider and hook. It resolves each flag from the
 * registry defaults, the gateway's `/api/features` response, and local
 * overrides kept in `localStorage`, and exposes route-visibility and debug
 * helpers to components.
 */

import { createQuery } from "@tanstack/solid-query";
import type { Accessor, ParentComponent } from "solid-js";
import {
  createContext,
  createEffect,
  createMemo,
  createSignal,
  onCleanup,
  useContext,
} from "solid-js";

import { fetchRuntimeFeatureFlags } from "@/lib/api/gateway";
import { isBoolean, isJsonObject } from "@/lib/api/json-value";
import {
  type OverrideMap,
  resolveFeatureFlags,
} from "@/lib/feature-flags/policy";
import type { FeatureFlagName } from "@/lib/feature-flags/registry";
import { FEATURE_FLAGS } from "@/lib/feature-flags/registry";

const OVERRIDE_STORAGE_KEY = "axinite.feature-flag-overrides";

type FeatureFlagContextValue = {
  overrides: Accessor<OverrideMap>;
  resolvedFlags: Accessor<Record<FeatureFlagName, boolean>>;
  setOverride: (name: FeatureFlagName, value: boolean) => void;
  clearOverride: (name: FeatureFlagName) => void;
  isRouteVisible: (name: FeatureFlagName) => boolean;
  isDebugEnabled: () => boolean;
};

function readOverrides(): OverrideMap {
  try {
    const raw = globalThis.localStorage.getItem(OVERRIDE_STORAGE_KEY);
    if (!raw) {
      return {};
    }
    const payload: unknown = JSON.parse(raw);
    if (!isJsonObject(payload)) {
      throw new TypeError("Feature-flag overrides must be an object");
    }
    const overrides: OverrideMap = {};
    for (const flag of FEATURE_FLAGS) {
      const value = payload[flag.name];
      if (isBoolean(value)) {
        overrides[flag.name] = value;
      }
    }
    return overrides;
  } catch {
    console.error(
      "[feature-flags] Failed to hydrate local overrides; using defaults."
    );
    return {};
  }
}

function writeOverrides(overrides: OverrideMap): void {
  try {
    globalThis.localStorage.setItem(
      OVERRIDE_STORAGE_KEY,
      JSON.stringify(overrides)
    );
  } catch {
    console.error(
      "[feature-flags] Failed to persist local overrides; changes apply only to this page."
    );
  }
}

// Debug flag surfacing is enabled in dev builds or when the `debug-flags=1`
// query parameter is present.
function isDebugFlagsEnabled(): boolean {
  const searchParams = new URLSearchParams(globalThis.location.search);
  return import.meta.env.DEV || searchParams.get("debug-flags") === "1";
}

const FeatureFlagContext = createContext<FeatureFlagContextValue>();

/**
 * Provide the feature-flag context: runtime flags from the gateway merged
 * with local overrides, which are persisted in `localStorage`.
 */
export const FeatureFlagProvider: ParentComponent = (props) => {
  const [overrides, setOverrides] = createSignal<OverrideMap>(readOverrides());

  const runtimeFlags = createQuery(() => ({
    queryKey: ["feature-flags"],
    queryFn: fetchRuntimeFeatureFlags,
  }));

  createEffect(() => {
    if (runtimeFlags.error) {
      console.error(
        "[feature-flags] Failed to load runtime flags; using local overrides and defaults."
      );
    }
  });

  const resolvedFlags = createMemo(() =>
    resolveFeatureFlags(runtimeFlags.data ?? {}, overrides())
  );

  const syncOverrides = () => setOverrides(readOverrides());

  globalThis.addEventListener("storage", syncOverrides);
  onCleanup(() => globalThis.removeEventListener("storage", syncOverrides));

  const setOverride = (name: FeatureFlagName, value: boolean) => {
    const next = { ...overrides(), [name]: value };
    setOverrides(next);
    writeOverrides(next);
  };

  const clearOverride = (name: FeatureFlagName) => {
    const next = { ...overrides() };
    delete next[name];
    setOverrides(next);
    writeOverrides(next);
  };

  return (
    <FeatureFlagContext.Provider
      value={{
        overrides,
        resolvedFlags,
        setOverride,
        clearOverride,
        isRouteVisible: (name) => resolvedFlags()[name],
        isDebugEnabled: isDebugFlagsEnabled,
      }}
    >
      {props.children}
    </FeatureFlagContext.Provider>
  );
};

/** Read the feature-flag context; throws when `FeatureFlagProvider` is missing. */
export function useFeatureFlags(): FeatureFlagContextValue {
  const context = useContext(FeatureFlagContext);

  if (!context) {
    throw new Error("FeatureFlagProvider is missing");
  }

  return context;
}
