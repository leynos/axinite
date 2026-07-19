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
import {
  FEATURE_FLAGS,
  type FeatureFlagName,
  getFeatureFlagDefaults,
} from "@/lib/feature-flags/registry";

const OVERRIDE_STORAGE_KEY = "axinite.feature-flag-overrides";

type OverrideMap = Partial<Record<FeatureFlagName, boolean>>;

type FeatureFlagContextValue = {
  overrides: Accessor<OverrideMap>;
  resolvedFlags: Accessor<Record<FeatureFlagName, boolean>>;
  setOverride: (name: FeatureFlagName, value: boolean) => void;
  clearOverride: (name: FeatureFlagName) => void;
  isRouteVisible: (name: FeatureFlagName) => boolean;
  isDebugEnabled: () => boolean;
};

function readOverrides(): OverrideMap {
  if (typeof window === "undefined") {
    return {};
  }

  const raw = window.localStorage.getItem(OVERRIDE_STORAGE_KEY);
  if (!raw) {
    return {};
  }

  try {
    return JSON.parse(raw) as OverrideMap;
  } catch {
    return {};
  }
}

function writeOverrides(overrides: OverrideMap): void {
  if (typeof window === "undefined") {
    return;
  }

  window.localStorage.setItem(OVERRIDE_STORAGE_KEY, JSON.stringify(overrides));
}

export function resolveFeatureFlags(
  serverFlags: Record<string, boolean>,
  overrides: OverrideMap
): Record<FeatureFlagName, boolean> {
  const defaults = getFeatureFlagDefaults();

  for (const flag of FEATURE_FLAGS) {
    const serverValue = serverFlags[flag.name];
    const overrideValue = overrides[flag.name];

    if (typeof overrideValue === "boolean") {
      defaults[flag.name] = overrideValue;
      continue;
    }

    if (typeof serverValue === "boolean") {
      defaults[flag.name] = serverValue;
    }
  }

  return defaults;
}

const FeatureFlagContext = createContext<FeatureFlagContextValue>();

export const FeatureFlagProvider: ParentComponent = (props) => {
  const [overrides, setOverrides] = createSignal<OverrideMap>(readOverrides());

  const runtimeFlags = createQuery(() => ({
    queryKey: ["feature-flags"],
    queryFn: fetchRuntimeFeatureFlags,
  }));

  createEffect(() => {
    if (runtimeFlags.error && (import.meta.env.DEV || isDebugEnabled())) {
      console.error(
        "[feature-flags] Failed to load runtime flags",
        runtimeFlags.error
      );
    }
  });

  const resolvedFlags = createMemo(() =>
    resolveFeatureFlags(runtimeFlags.data ?? {}, overrides())
  );

  const syncOverrides = () => setOverrides(readOverrides());

  if (typeof window !== "undefined") {
    window.addEventListener("storage", syncOverrides);
    onCleanup(() => window.removeEventListener("storage", syncOverrides));
  }

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

  const isDebugEnabled = () => {
    if (typeof window === "undefined") {
      return false;
    }

    const searchParams = new URLSearchParams(window.location.search);
    return import.meta.env.DEV || searchParams.get("debug-flags") === "1";
  };

  return (
    <FeatureFlagContext.Provider
      value={{
        overrides,
        resolvedFlags,
        setOverride,
        clearOverride,
        isRouteVisible: (name) => resolvedFlags()[name],
        isDebugEnabled,
      }}
    >
      {props.children}
    </FeatureFlagContext.Provider>
  );
};

export function useFeatureFlags(): FeatureFlagContextValue {
  const context = useContext(FeatureFlagContext);

  if (!context) {
    throw new Error("FeatureFlagProvider is missing");
  }

  return context;
}
