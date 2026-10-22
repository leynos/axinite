// @vitest-environment jsdom
// happy-dom drops a bare `<tr>` when Solid clones its row template, so the
// routines table cannot render under the suite's default environment.

/**
 * @file Behaviour tests for the routines route (`RoutinesPreview`), covering
 * the routines table and detail view against in-process gateway responses.
 * Runs under jsdom rather than the suite's default happy-dom (see above).
 */

import { render, screen, waitFor } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeAll, beforeEach, describe, expect, it } from "vitest";

import { RoutinesPreview } from "@/components/routines-preview";
import type {
  RoutineDetailResponse,
  RoutineInfo,
  RoutineRunsResponse,
} from "@/lib/api/contracts";
import { isBoolean, isJsonObject } from "@/lib/api/json-value";
import { DEFAULT_LOCALE } from "@/lib/i18n/supported-locales";
import { GatewayTestServer } from "./support/gateway-test-server";
import {
  setApiRequestHandler,
  setupI18nTestHarness,
} from "./support/i18n-test-runtime";
import { TestProviders } from "./support/test-providers";

let gateway: GatewayTestServer;

function makeRoutine(id: string, enabled: boolean): RoutineInfo {
  return {
    id,
    name: `Routine ${id}`,
    description: `Does ${id}`,
    enabled,
    trigger_type: "cron",
    trigger_summary: "0 9 * * *",
    action_type: "lightweight",
    last_run_at: null,
    next_fire_at: null,
    run_count: 0,
    consecutive_failures: 0,
    status: enabled ? "active" : "disabled",
  };
}

function makeDetail(id: string, enabled: boolean): RoutineDetailResponse {
  return {
    id,
    name: `Routine ${id}`,
    description: `Does ${id}`,
    enabled,
    trigger: {},
    action: {},
    guardrails: {},
    notify: {},
    last_run_at: null,
    next_fire_at: null,
    run_count: 0,
    consecutive_failures: 0,
    created_at: "2026-10-01T09:00:00Z",
    recent_runs: [],
  };
}

let details: Map<string, RoutineDetailResponse>;
let routineList: RoutineInfo[];
let routineRuns: Map<string, RoutineRunsResponse["runs"]>;

function renderRoutines() {
  return render(() => (
    <TestProviders>
      <RoutinesPreview />
    </TestProviders>
  ));
}

beforeAll(async () => {
  await setupI18nTestHarness();
});

beforeEach(async () => {
  globalThis.localStorage.clear();
  const runtime = await import("@/lib/i18n/runtime");
  await runtime.default.changeLanguage(DEFAULT_LOCALE);

  details = new Map([
    ["alpha", makeDetail("alpha", true)],
    ["beta", makeDetail("beta", false)],
  ]);
  routineList = [makeRoutine("alpha", true), makeRoutine("beta", false)];
  routineRuns = new Map();
  gateway = new GatewayTestServer()
    .route("GET", "/api/routines", async () =>
      Response.json({ routines: routineList })
    )
    .respond("GET", "/api/routines/summary", {
      total: 2,
      enabled: 1,
      disabled: 1,
      failing: 0,
      runs_today: 0,
    })
    .route("GET", /^\/api\/routines\/[^/]+$/u, async (request) => {
      const id = new URL(request.url).pathname.split("/").at(-1) ?? "";
      const detail = details.get(id);
      return detail
        ? Response.json(detail)
        : new Response("Routine not found", { status: 404 });
    })
    .route("GET", /^\/api\/routines\/[^/]+\/runs$/u, async (request) => {
      const id = new URL(request.url).pathname.split("/").at(-2) ?? "";
      return Response.json({ runs: routineRuns.get(id) ?? [] });
    })
    .route("POST", /^\/api\/routines\/[^/]+\/trigger$/u, async (request) => {
      const id = new URL(request.url).pathname.split("/").at(-2) ?? "";
      routineRuns.set(id, [
        {
          id: "new-run",
          trigger_type: "manual",
          started_at: "2026-10-01T09:00:00Z",
          completed_at: null,
          status: "completed",
          result_summary: "Fresh manual run",
          tokens_used: 0,
          job_id: null,
        },
      ]);
      return Response.json({ success: true, message: "Triggered" });
    })
    .route("POST", /^\/api\/routines\/[^/]+\/toggle$/u, async (request) => {
      const id = new URL(request.url).pathname.split("/").at(-2) ?? "";
      const body: unknown = await request.json();
      if (!isJsonObject(body) || !isBoolean(body["enabled"])) {
        return new Response("Invalid toggle request", { status: 400 });
      }
      const enabled = body["enabled"];
      details.set(id, makeDetail(id, enabled));
      routineList = routineList.map((routine) =>
        routine.id === id ? makeRoutine(id, enabled) : routine
      );
      return Response.json({ success: true, message: "Toggled" });
    })
    .route("DELETE", /^\/api\/routines\/[^/]+$/u, async (request) => {
      const id = new URL(request.url).pathname.split("/").at(-1) ?? "";
      routineList = routineList.filter((routine) => routine.id !== id);
      details.delete(id);
      return Response.json({ success: true, message: "Deleted" });
    });
});

afterEach(() => setApiRequestHandler(undefined));

describe("routines preview behaviour", () => {
  it("selects the first routine and loads its detail and runs", async () => {
    renderRoutines();

    await waitFor(() => {
      expect(
        gateway.requests.some(
          (request) => request.path === "/api/routines/alpha"
        )
      ).toBe(true);
    });
    expect(
      gateway.requests.some(
        (request) => request.path === "/api/routines/alpha/runs"
      )
    ).toBe(true);
    expect(
      await screen.findByRole("heading", { name: "Routine alpha" })
    ).toBeVisible();
  });

  it("switches the detail pane to the routine that is clicked", async () => {
    renderRoutines();
    await screen.findByRole("button", { name: "Routine beta" });

    await userEvent.click(screen.getByRole("button", { name: "Routine beta" }));

    await waitFor(() => {
      expect(
        gateway.requests.some(
          (request) => request.path === "/api/routines/beta"
        )
      ).toBe(true);
    });
    expect(
      await screen.findByRole("heading", { name: "Routine beta" })
    ).toBeVisible();
    expect(screen.queryByRole("heading", { name: "Routine alpha" })).toBeNull();
  });

  it("triggers the selected routine with Run now", async () => {
    renderRoutines();

    await userEvent.click(
      await screen.findByRole("button", { name: "Run now" })
    );

    await waitFor(() => {
      expect(
        gateway.requests.some(
          (request) =>
            request.method === "POST" &&
            request.path === "/api/routines/alpha/trigger"
        )
      ).toBe(true);
    });
    expect(await screen.findByText("Fresh manual run")).toBeVisible();
    expect(
      gateway.requests.filter((request) => request.path === "/api/routines")
    ).toHaveLength(2);
    expect(
      gateway.requests.filter(
        (request) => request.path === "/api/routines/summary"
      )
    ).toHaveLength(2);
  });

  it("disables an enabled routine", async () => {
    renderRoutines();

    await userEvent.click(
      await screen.findByRole("button", { name: "Disable" })
    );

    await waitFor(() => {
      expect(
        gateway.requests.some(
          (request) =>
            request.method === "POST" &&
            request.path === "/api/routines/alpha/toggle" &&
            request.body?.["enabled"] === false
        )
      ).toBe(true);
    });
    expect(await screen.findByRole("button", { name: "Enable" })).toBeVisible();
    expect(
      gateway.requests.filter((request) => request.path === "/api/routines")
    ).toHaveLength(2);
  });

  it("enables a disabled routine", async () => {
    renderRoutines();
    await userEvent.click(
      await screen.findByRole("button", { name: "Routine beta" })
    );

    await userEvent.click(
      await screen.findByRole("button", { name: "Enable" })
    );

    await waitFor(() => {
      expect(
        gateway.requests.some(
          (request) =>
            request.method === "POST" &&
            request.path === "/api/routines/beta/toggle" &&
            request.body?.["enabled"] === true
        )
      ).toBe(true);
    });
    expect(
      await screen.findByRole("button", { name: "Disable" })
    ).toBeVisible();
    expect(
      gateway.requests.filter((request) => request.path === "/api/routines")
    ).toHaveLength(2);
  });

  it("deletes the selected routine and selects its remaining replacement", async () => {
    renderRoutines();
    await screen.findByRole("button", { name: "Delete" });
    const listCalls = gateway.requests.filter(
      (request) => request.path === "/api/routines"
    ).length;

    await userEvent.click(screen.getByRole("button", { name: "Delete" }));

    await waitFor(() => {
      expect(
        gateway.requests.some(
          (request) =>
            request.method === "DELETE" &&
            request.path === "/api/routines/alpha"
        )
      ).toBe(true);
    });
    await waitFor(() => {
      expect(
        gateway.requests.filter((request) => request.path === "/api/routines")
          .length
      ).toBeGreaterThan(listCalls);
    });
    expect(
      await screen.findByRole("heading", { name: "Routine beta" })
    ).toBeVisible();
    expect(screen.queryByRole("button", { name: "Routine alpha" })).toBeNull();
  });

  it("clears the detail and selection when the last routine is deleted", async () => {
    routineList = [makeRoutine("alpha", true)];
    renderRoutines();
    await userEvent.click(
      await screen.findByRole("button", { name: "Delete" })
    );
    await waitFor(() => {
      expect(screen.queryByRole("button", { name: "Delete" })).toBeNull();
      expect(
        screen.queryByRole("heading", { name: "Routine alpha" })
      ).toBeNull();
      expect(
        screen.queryByRole("button", { name: "Routine alpha" })
      ).toBeNull();
    });
  });

  it.each([
    ["Run now", "triggerRoutine"],
    ["Disable", "toggleRoutine"],
    ["Delete", "deleteRoutine"],
  ] as const)("shows a failed %s action without refreshing or clearing selection", async (button, mutation) => {
    const action = {
      triggerRoutine: ["POST", "/api/routines/alpha/trigger"],
      toggleRoutine: ["POST", "/api/routines/alpha/toggle"],
      deleteRoutine: ["DELETE", "/api/routines/alpha"],
    } as const;
    const [method, path] = action[mutation];
    gateway.fail(method, path, 500, "Gateway offline");
    renderRoutines();
    await userEvent.click(await screen.findByRole("button", { name: button }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The routine action failed. Try again."
    );
    expect(
      screen.getByRole("heading", { name: "Routine alpha" })
    ).toBeVisible();
    expect(
      gateway.requests.filter((request) => request.path === "/api/routines")
    ).toHaveLength(1);
  });
});
