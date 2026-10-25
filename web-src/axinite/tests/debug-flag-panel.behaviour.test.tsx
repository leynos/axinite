// @vitest-environment jsdom
/**
 * @file Debug controls exercise the real runtime provider and query/i18n
 * providers. Only fetch is replaced; the shell's actual debug predicate gates
 * the panel without starting unrelated shell status polling or SSE streams.
 */

import { render, screen, waitFor, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { Show } from "solid-js";
import {
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from "vitest";

import { DebugFlagPanel } from "@/components/debug-flag-panel";
import { FEATURE_FLAGS } from "@/lib/feature-flags/registry";
import { useFeatureFlags } from "@/lib/feature-flags/runtime";
import { DEFAULT_LOCALE } from "@/lib/i18n/supported-locales";
import { setupI18nTestHarness } from "./support/i18n-test-runtime";
import { TestProviders } from "./support/test-providers";

const storageKey = "axinite.feature-flag-overrides";
const runtimeFlags = Object.fromEntries(
  FEATURE_FLAGS.map((flag, index) => [flag.name, index % 2 === 0])
);
let originalFetch: typeof fetch;

function DebugShellGate() {
  const flags = useFeatureFlags();
  return (
    <Show when={flags.isDebugEnabled()}>
      <DebugFlagPanel />
    </Show>
  );
}

function mountPanel() {
  return render(() => (
    <TestProviders>
      <DebugShellGate />
    </TestProviders>
  ));
}

function flagRow(name: string) {
  return within(screen.getByRole("row", { name: new RegExp(`^${name}\\b`) }));
}

function flagRowAt(rows: HTMLElement[], index: number) {
  const row = rows[index];
  if (!row) {
    throw new Error(`Missing rendered feature flag row at index ${index}`);
  }
  return within(row);
}

function storedOverrides(): Record<string, boolean> {
  return JSON.parse(globalThis.localStorage.getItem(storageKey) ?? "{}");
}

async function openPanel() {
  await userEvent.click(
    await screen.findByRole("button", { name: "Toggle overrides" })
  );
  await screen.findByRole("table", { name: "Feature flag overrides" });
  const rows = screen.getAllByRole("row").slice(1);
  expect(rows).toHaveLength(FEATURE_FLAGS.length);
  await waitFor(() => {
    for (const [index, flag] of FEATURE_FLAGS.entries()) {
      expect(
        flagRowAt(rows, index).getByText(runtimeFlags[flag.name] ? "On" : "Off")
      ).toBeVisible();
    }
  });
}

beforeAll(async () => {
  await setupI18nTestHarness();
  originalFetch = globalThis.fetch;
});
beforeEach(async () => {
  vi.stubEnv("DEV", false);
  globalThis.localStorage.clear();
  globalThis.history.replaceState(null, "", "/?debug-flags=1");
  vi.stubGlobal(
    "fetch",
    vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      if (String(input) === "/api/features") {
        return Promise.resolve(Response.json(runtimeFlags));
      }
      return originalFetch(input, init);
    })
  );
  const runtime = await import("@/lib/i18n/runtime");
  await runtime.default.changeLanguage(DEFAULT_LOCALE);
});
afterEach(() => {
  globalThis.localStorage.clear();
  globalThis.history.replaceState(null, "", "/");
  vi.unstubAllEnvs();
  vi.unstubAllGlobals();
});

describe("debug flag panel with the real runtime", () => {
  it("is absent in production unless the exact debug query opt-in is present", () => {
    globalThis.history.replaceState(null, "", "/?debug-flags=0");
    mountPanel();
    expect(screen.queryByRole("heading", { name: "Feature flags" })).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Toggle overrides" })
    ).toBeNull();
  });

  it("enables from debug-flags=1 and displays every registry flag's runtime value", async () => {
    mountPanel();
    expect(
      await screen.findByRole("heading", { name: "Feature flags" })
    ).toBeVisible();
    await openPanel();
    const rows = screen.getAllByRole("row");
    expect(rows).toHaveLength(FEATURE_FLAGS.length + 1);
    const flagRows = rows.slice(1);
    for (const [index, flag] of FEATURE_FLAGS.entries()) {
      const row = flagRowAt(flagRows, index);
      expect(
        row.getByText(`${flag.owner} · ${flag.backendContract}`)
      ).toBeVisible();
      expect(row.getByRole("button", { name: "Clear" })).toBeDisabled();
    }
  });

  it.each(
    FEATURE_FLAGS
  )("forces and clears $name and persists each override", async (flag) => {
    mountPanel();
    await openPanel();
    const row = flagRow(flag.name);
    await userEvent.click(row.getByRole("button", { name: "Force on" }));
    expect(row.getByText("On")).toBeVisible();
    expect(row.getByRole("button", { name: "Clear" })).toBeEnabled();
    expect(storedOverrides()).toEqual({ [flag.name]: true });

    await userEvent.click(row.getByRole("button", { name: "Force off" }));
    expect(row.getByText("Off")).toBeVisible();
    expect(storedOverrides()).toEqual({ [flag.name]: false });

    await userEvent.click(row.getByRole("button", { name: "Clear" }));
    expect(row.getByText(runtimeFlags[flag.name] ? "On" : "Off")).toBeVisible();
    expect(row.getByRole("button", { name: "Clear" })).toBeDisabled();
    expect(storedOverrides()).toEqual({});
  });

  it("hydrates persisted overrides in a new provider and retains unrelated values on clear", async () => {
    const first = mountPanel();
    await openPanel();
    await userEvent.click(
      flagRow("route_chat").getByRole("button", { name: "Force off" })
    );
    await userEvent.click(
      flagRow("route_jobs").getByRole("button", { name: "Force off" })
    );
    first.unmount();

    mountPanel();
    await userEvent.click(
      await screen.findByRole("button", { name: "Toggle overrides" })
    );
    expect(flagRow("route_chat").getByText("Off")).toBeVisible();
    expect(
      flagRow("route_chat").getByRole("button", { name: "Clear" })
    ).toBeEnabled();
    await userEvent.click(
      flagRow("route_chat").getByRole("button", { name: "Clear" })
    );
    await waitFor(() =>
      expect(flagRow("route_chat").getByText("On")).toBeVisible()
    );
    expect(storedOverrides()).toEqual({ route_jobs: false });
    expect(flagRow("route_jobs").getByText("Off")).toBeVisible();
  });
});
