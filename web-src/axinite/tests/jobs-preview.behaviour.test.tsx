import { render, screen, waitFor } from "@solidjs/testing-library";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { AppProviders } from "@/app/providers";
import { JobsPreview } from "@/components/jobs-preview";
import { DEFAULT_LOCALE } from "@/lib/i18n/supported-locales";
import { setupI18nTestHarness } from "./support/i18n-test-runtime";

const jobsApiMocks = vi.hoisted(() => ({
  fetchJobs: vi.fn(),
  fetchJobSummary: vi.fn(),
  fetchJobDetail: vi.fn(),
  fetchJobEvents: vi.fn(),
  fetchJobFiles: vi.fn(),
  readJobFile: vi.fn(),
  restartJob: vi.fn(),
  cancelJob: vi.fn(),
  promptJob: vi.fn(),
}));

vi.mock("@/lib/api/jobs", () => jobsApiMocks);

beforeAll(async () => {
  await setupI18nTestHarness();
});

beforeEach(async () => {
  for (const mock of Object.values(jobsApiMocks)) {
    mock.mockReset();
  }
  window.localStorage.clear();
  const runtime = await import("@/lib/i18n/runtime");
  await runtime.default.changeLanguage(DEFAULT_LOCALE);

  jobsApiMocks.fetchJobSummary.mockResolvedValue({
    total: 0,
    running: 0,
    completed: 0,
    failed: 0,
    stuck: 0,
  });
});

describe("jobs preview error handling", () => {
  it("shows a visible error state when the jobs list request fails", async () => {
    jobsApiMocks.fetchJobs.mockRejectedValue(
      new Error("Simulated failure for /api/jobs.")
    );

    render(() => (
      <AppProviders>
        <JobsPreview />
      </AppProviders>
    ));

    await waitFor(
      () => {
        expect(screen.getByRole("alert")).toBeVisible();
        expect(screen.getByRole("alert").textContent).toContain(
          "Jobs could not be loaded"
        );
      },
      { timeout: 5_000 }
    );
  });
});
