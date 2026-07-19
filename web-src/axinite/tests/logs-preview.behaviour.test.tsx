import { render, screen, waitFor } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { AppProviders } from "@/app/providers";
import { LogsPreview } from "@/components/logs-preview";
import type { LogEntry } from "@/lib/api/contracts";
import { DEFAULT_LOCALE } from "@/lib/i18n/supported-locales";
import { setupI18nTestHarness } from "./support/i18n-test-runtime";

const logsApiMocks = vi.hoisted(() => ({
  fetchLogLevel: vi.fn(),
  setLogLevel: vi.fn(),
  connectLogEvents: vi.fn(),
  listener: null as ((entry: LogEntry) => void) | null,
}));

vi.mock("@/lib/api/logs", () => ({
  connectLogEvents: logsApiMocks.connectLogEvents,
  fetchLogLevel: logsApiMocks.fetchLogLevel,
  setLogLevel: logsApiMocks.setLogLevel,
}));

function makeEntry(overrides: Partial<LogEntry> = {}): LogEntry {
  return {
    level: "info",
    target: "axinite::gateway",
    message: "Gateway ready",
    timestamp: "2026-03-26T12:00:00Z",
    ...overrides,
  };
}

beforeAll(async () => {
  await setupI18nTestHarness();
});

beforeEach(async () => {
  logsApiMocks.listener = null;
  logsApiMocks.fetchLogLevel.mockReset();
  logsApiMocks.setLogLevel.mockReset();
  logsApiMocks.connectLogEvents.mockReset();

  window.localStorage.clear();
  const runtime = await import("@/lib/i18n/runtime");
  await runtime.default.changeLanguage(DEFAULT_LOCALE);

  logsApiMocks.fetchLogLevel.mockResolvedValue({ level: "info" });
  logsApiMocks.setLogLevel.mockImplementation((level: string) =>
    Promise.resolve({ level })
  );
  logsApiMocks.connectLogEvents.mockImplementation(
    (listener: (entry: LogEntry) => void) => {
      logsApiMocks.listener = listener;
      return { close: vi.fn() };
    }
  );
});

async function renderLogsPreview() {
  render(() => (
    <AppProviders>
      <LogsPreview />
    </AppProviders>
  ));

  await waitFor(() => {
    expect(logsApiMocks.connectLogEvents).toHaveBeenCalled();
  });
}

describe("logs preview behaviour", () => {
  it("renders streamed entries", async () => {
    await renderLogsPreview();

    logsApiMocks.listener?.(
      makeEntry({ message: "Gateway ready", target: "axinite::gateway" })
    );

    await waitFor(() => {
      expect(screen.getByText(/Gateway ready/)).toBeVisible();
    });
    expect(screen.getByText(/axinite::gateway/)).toBeVisible();
  });

  it("hides entries below the selected display level", async () => {
    await renderLogsPreview();

    const filterSelect = screen.getByLabelText("Display level");
    await userEvent.selectOptions(filterSelect, "warn");

    logsApiMocks.listener?.(
      makeEntry({ level: "info", message: "Ignored info entry" })
    );
    logsApiMocks.listener?.(
      makeEntry({ level: "warn", message: "Visible warn entry" })
    );

    await waitFor(() => {
      expect(screen.getByText(/Visible warn entry/)).toBeVisible();
    });
    expect(screen.queryByText(/Ignored info entry/)).toBeNull();
  });

  it("filters entries by a target substring", async () => {
    await renderLogsPreview();

    const targetInput = screen.getByLabelText("Target filter");
    await userEvent.type(targetInput, "chat");

    logsApiMocks.listener?.(
      makeEntry({ target: "axinite::chat", message: "Chat entry" })
    );
    logsApiMocks.listener?.(
      makeEntry({ target: "axinite::jobs", message: "Jobs entry" })
    );

    await waitFor(() => {
      expect(screen.getByText(/Chat entry/)).toBeVisible();
    });
    expect(screen.queryByText(/Jobs entry/)).toBeNull();
  });

  it("stops appending entries while paused and resumes on demand", async () => {
    await renderLogsPreview();

    await userEvent.click(screen.getByRole("button", { name: "Pause" }));

    logsApiMocks.listener?.(makeEntry({ message: "Missed while paused" }));

    await waitFor(() => {
      expect(screen.queryByText(/Missed while paused/)).toBeNull();
    });

    await userEvent.click(screen.getByRole("button", { name: "Resume" }));

    logsApiMocks.listener?.(makeEntry({ message: "Visible after resume" }));

    await waitFor(() => {
      expect(screen.getByText(/Visible after resume/)).toBeVisible();
    });
  });

  it("clears rendered entries", async () => {
    await renderLogsPreview();

    logsApiMocks.listener?.(makeEntry({ message: "Entry to clear" }));

    await waitFor(() => {
      expect(screen.getByText(/Entry to clear/)).toBeVisible();
    });

    await userEvent.click(screen.getByRole("button", { name: "Clear" }));

    expect(screen.queryByText(/Entry to clear/)).toBeNull();
  });

  it("calls setLogLevel when the log level select changes", async () => {
    await renderLogsPreview();

    const levelSelect = screen.getByLabelText("Log level");
    await userEvent.selectOptions(levelSelect, "debug");

    await waitFor(() => {
      expect(logsApiMocks.setLogLevel).toHaveBeenCalledWith("debug");
    });
  });
});
