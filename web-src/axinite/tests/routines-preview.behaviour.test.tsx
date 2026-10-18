// @vitest-environment jsdom
// happy-dom drops a bare `<tr>` when Solid clones its row template, so the
// routines table cannot render under the suite's default environment.

/**
 * @file Behaviour tests for the routines route (`RoutinesPreview`), covering
 * the routines table and detail view against mocked data. Runs under jsdom
 * rather than the suite's default happy-dom (see above).
 */

import { render, screen, waitFor } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { RoutinesPreview } from "@/components/routines-preview";
import type { RoutineDetailResponse, RoutineInfo } from "@/lib/api/contracts";
import { DEFAULT_LOCALE } from "@/lib/i18n/supported-locales";
import { setupI18nTestHarness } from "./support/i18n-test-runtime";
import { TestProviders } from "./support/test-providers";

const routinesApiMocks = vi.hoisted(() => ({
  fetchRoutines: vi.fn(),
  fetchRoutineSummary: vi.fn(),
  fetchRoutineDetail: vi.fn(),
  fetchRoutineRuns: vi.fn(),
  triggerRoutine: vi.fn(),
  toggleRoutine: vi.fn(),
  deleteRoutine: vi.fn(),
}));

vi.mock("@/lib/api/routines", () => routinesApiMocks);

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

let details: Record<string, RoutineDetailResponse>;
let routineList: RoutineInfo[];

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
  for (const mock of Object.values(routinesApiMocks)) {
    mock.mockReset();
  }
  window.localStorage.clear();
  const runtime = await import("@/lib/i18n/runtime");
  await runtime.default.changeLanguage(DEFAULT_LOCALE);

  details = {
    alpha: makeDetail("alpha", true),
    beta: makeDetail("beta", false),
  };
  routineList = [makeRoutine("alpha", true), makeRoutine("beta", false)];
  routinesApiMocks.fetchRoutines.mockImplementation(async () => ({
    routines: routineList,
  }));
  routinesApiMocks.fetchRoutineSummary.mockResolvedValue({
    total: 2,
    enabled: 1,
    disabled: 1,
    failing: 0,
    runs_today: 0,
  });
  routinesApiMocks.fetchRoutineDetail.mockImplementation(async (id: string) => {
    const detail = details[id];
    if (!detail) {
      throw new Error("Routine not found");
    }
    return detail;
  });
  routinesApiMocks.fetchRoutineRuns.mockResolvedValue({ runs: [] });
  routinesApiMocks.triggerRoutine.mockResolvedValue({
    success: true,
    message: "Triggered",
  });
  routinesApiMocks.toggleRoutine.mockImplementation(
    async (id: string, request: { enabled: boolean }) => {
      details[id] = { ...makeDetail(id, request.enabled) };
      routineList = routineList.map((routine) =>
        routine.id === id ? makeRoutine(id, request.enabled) : routine
      );
      return { success: true, message: "Toggled" };
    }
  );
  routinesApiMocks.deleteRoutine.mockImplementation(async (id: string) => {
    routineList = routineList.filter((routine) => routine.id !== id);
    delete details[id];
    return { success: true, message: "Deleted" };
  });
});

describe("routines preview behaviour", () => {
  it("selects the first routine and loads its detail and runs", async () => {
    renderRoutines();

    await waitFor(() => {
      expect(routinesApiMocks.fetchRoutineDetail).toHaveBeenCalledWith("alpha");
    });
    expect(routinesApiMocks.fetchRoutineRuns).toHaveBeenCalledWith("alpha");
    expect(
      await screen.findByRole("heading", { name: "Routine alpha" })
    ).toBeVisible();
  });

  it("switches the detail pane to the routine that is clicked", async () => {
    renderRoutines();
    await screen.findByRole("button", { name: "Routine beta" });

    await userEvent.click(screen.getByRole("button", { name: "Routine beta" }));

    await waitFor(() => {
      expect(routinesApiMocks.fetchRoutineDetail).toHaveBeenCalledWith("beta");
    });
    expect(
      await screen.findByRole("heading", { name: "Routine beta" })
    ).toBeVisible();
    expect(screen.queryByRole("heading", { name: "Routine alpha" })).toBeNull();
  });

  it("triggers the selected routine with Run now", async () => {
    routinesApiMocks.triggerRoutine.mockImplementation(async () => {
      routinesApiMocks.fetchRoutineRuns.mockResolvedValue({
        runs: [
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
        ],
      });
      return { success: true, message: "Triggered" };
    });
    renderRoutines();

    await userEvent.click(
      await screen.findByRole("button", { name: "Run now" })
    );

    await waitFor(() => {
      expect(routinesApiMocks.triggerRoutine).toHaveBeenCalledWith("alpha");
    });
    expect(await screen.findByText("Fresh manual run")).toBeVisible();
    expect(routinesApiMocks.fetchRoutines).toHaveBeenCalledTimes(2);
    expect(routinesApiMocks.fetchRoutineSummary).toHaveBeenCalledTimes(2);
  });

  it("disables an enabled routine", async () => {
    renderRoutines();

    await userEvent.click(
      await screen.findByRole("button", { name: "Disable" })
    );

    await waitFor(() => {
      expect(routinesApiMocks.toggleRoutine).toHaveBeenCalledWith("alpha", {
        enabled: false,
      });
    });
    expect(await screen.findByRole("button", { name: "Enable" })).toBeVisible();
    expect(routinesApiMocks.fetchRoutines).toHaveBeenCalledTimes(2);
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
      expect(routinesApiMocks.toggleRoutine).toHaveBeenCalledWith("beta", {
        enabled: true,
      });
    });
    expect(
      await screen.findByRole("button", { name: "Disable" })
    ).toBeVisible();
    expect(routinesApiMocks.fetchRoutines).toHaveBeenCalledTimes(2);
  });

  it("deletes the selected routine and selects its remaining replacement", async () => {
    renderRoutines();
    await screen.findByRole("button", { name: "Delete" });
    const listCalls = routinesApiMocks.fetchRoutines.mock.calls.length;

    await userEvent.click(screen.getByRole("button", { name: "Delete" }));

    await waitFor(() => {
      expect(routinesApiMocks.deleteRoutine).toHaveBeenCalledWith("alpha");
    });
    await waitFor(() => {
      expect(routinesApiMocks.fetchRoutines.mock.calls.length).toBeGreaterThan(
        listCalls
      );
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
    routinesApiMocks[mutation].mockRejectedValue(new Error("Gateway offline"));
    renderRoutines();
    await userEvent.click(await screen.findByRole("button", { name: button }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The routine action failed. Try again."
    );
    expect(
      screen.getByRole("heading", { name: "Routine alpha" })
    ).toBeVisible();
    expect(routinesApiMocks.fetchRoutines).toHaveBeenCalledTimes(1);
  });
});
