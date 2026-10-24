/**
 * @file Behaviour tests for the logs route (`LogsPreview`) using real API
 * requests and controlled in-process gateway responses and SSE frames.
 */

import { render, screen, waitFor } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeAll, beforeEach, describe, expect, it } from "vitest";

import { AppProviders } from "@/app/providers";
import { LogsPreview } from "@/components/logs-preview";
import type { LogEntry } from "@/lib/api/contracts";
import { DEFAULT_LOCALE } from "@/lib/i18n/supported-locales";
import {
  GatewayEventSource,
  GatewayTestServer,
} from "./support/gateway-test-server";
import {
  setApiRequestHandler,
  setupI18nTestHarness,
} from "./support/i18n-test-runtime";

let gateway: GatewayTestServer;

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
  gateway = new GatewayTestServer()
    .respond({
      method: "GET",
      path: "/api/logs/level",
      payload: { level: "info" },
    })
    .respond({
      method: "PUT",
      path: "/api/logs/level",
      payload: { level: "info" },
    });

  globalThis.localStorage.clear();
  const runtime = await import("@/lib/i18n/runtime");
  await runtime.default.changeLanguage(DEFAULT_LOCALE);
});

afterEach(() => setApiRequestHandler(undefined));

async function renderLogsPreview() {
  render(() => (
    <AppProviders>
      <LogsPreview />
    </AppProviders>
  ));

  await waitFor(() => expect(GatewayEventSource.latest).toBeDefined());
}

describe("logs preview behaviour", () => {
  it("renders streamed entries", async () => {
    await renderLogsPreview();

    GatewayEventSource.latest?.emit({
      type: "log",
      payload: makeEntry({
        message: "Gateway ready",
        target: "axinite::gateway",
      }),
    });

    await waitFor(() => {
      expect(screen.getByText(/Gateway ready/)).toBeVisible();
    });
    expect(screen.getByText(/axinite::gateway/)).toBeVisible();
  });

  it("hides entries below the selected display level", async () => {
    await renderLogsPreview();

    const filterSelect = screen.getByLabelText("Display level");
    await userEvent.selectOptions(filterSelect, "warn");

    GatewayEventSource.latest?.emit({
      type: "log",
      payload: makeEntry({ level: "info", message: "Ignored info entry" }),
    });
    GatewayEventSource.latest?.emit({
      type: "log",
      payload: makeEntry({ level: "warn", message: "Visible warn entry" }),
    });

    await waitFor(() => {
      expect(screen.getByText(/Visible warn entry/)).toBeVisible();
    });
    expect(screen.queryByText(/Ignored info entry/)).toBeNull();
  });

  it("filters entries by a target substring", async () => {
    await renderLogsPreview();

    const targetInput = screen.getByLabelText("Target filter");
    await userEvent.type(targetInput, "chat");

    GatewayEventSource.latest?.emit({
      type: "log",
      payload: makeEntry({ target: "axinite::chat", message: "Chat entry" }),
    });
    GatewayEventSource.latest?.emit({
      type: "log",
      payload: makeEntry({ target: "axinite::jobs", message: "Jobs entry" }),
    });

    await waitFor(() => {
      expect(screen.getByText(/Chat entry/)).toBeVisible();
    });
    expect(screen.queryByText(/Jobs entry/)).toBeNull();
  });

  it("stops appending entries while paused and resumes on demand", async () => {
    await renderLogsPreview();

    await userEvent.click(screen.getByRole("button", { name: "Pause" }));

    GatewayEventSource.latest?.emit({
      type: "log",
      payload: makeEntry({ message: "Missed while paused" }),
    });

    await waitFor(() => {
      expect(screen.queryByText(/Missed while paused/)).toBeNull();
    });

    await userEvent.click(screen.getByRole("button", { name: "Resume" }));

    GatewayEventSource.latest?.emit({
      type: "log",
      payload: makeEntry({ message: "Visible after resume" }),
    });

    await waitFor(() => {
      expect(screen.getByText(/Visible after resume/)).toBeVisible();
    });
  });

  it("clears rendered entries", async () => {
    await renderLogsPreview();

    GatewayEventSource.latest?.emit({
      type: "log",
      payload: makeEntry({ message: "Entry to clear" }),
    });

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
      expect(
        gateway.requests.some(
          (request) =>
            request.method === "PUT" &&
            request.path === "/api/logs/level" &&
            request.body?.["level"] === "debug"
        )
      ).toBe(true);
    });
  });
});
