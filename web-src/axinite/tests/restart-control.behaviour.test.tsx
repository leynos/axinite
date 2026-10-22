/**
 * @file Behaviour tests for `RestartControl`: whether it is shown, and how it
 * reflects the restart phases reported by the injected restart dependencies.
 */

import { render, screen, waitFor, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from "vitest";
import { AppProviders } from "@/app/providers";
import { RestartControl } from "@/components/restart-control";
import * as chatApi from "@/lib/api/chat";
import { RequestError } from "@/lib/api/client";
import { DEFAULT_LOCALE } from "@/lib/i18n/supported-locales";
import type {
  RestartDeps,
  RestartSendResult,
  RestartStreamHandlers,
} from "@/lib/restart";
import { GatewayEventSource } from "./support/gateway-test-server";
import { setupI18nTestHarness } from "./support/i18n-test-runtime";

const dialogStyles = document.createElement("style");

function rawFailureDeps(onPhase: RestartDeps["onPhase"]): RestartDeps {
  return {
    sendRestart: () =>
      Promise.resolve({ kind: "failed", category: "transport" }),
    onPhase,
    openStream: () => ({ close: () => {} }),
    checkStatus: () => Promise.resolve(true),
    scheduleStatusPolls: () => () => {},
  };
}

beforeAll(async () => {
  // Happy DOM omits the browser's initial animation-name value. Kobalte
  // otherwise waits for an animation-end event when these unanimated dialogs
  // close, retaining their pointer lock. Preserve real pointer checks below.
  dialogStyles.textContent =
    ".dialog-overlay, .dialog-surface { animation-name: none; }";
  document.head.append(dialogStyles);
  await setupI18nTestHarness();
});

afterAll(() => dialogStyles.remove());

beforeEach(async () => {
  globalThis.localStorage.clear();
  const runtime = await import("@/lib/i18n/runtime");
  await runtime.default.changeLanguage(DEFAULT_LOCALE);
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("restart control", () => {
  it("is hidden when the gateway does not permit restarts", async () => {
    render(() => (
      <AppProviders>
        <RestartControl restartEnabled={() => false} />
      </AppProviders>
    ));

    await Promise.resolve();
    expect(
      screen.queryByRole("button", { name: "Restart gateway" })
    ).toBeNull();
  });

  it("confirms, sends /restart, and reports completion on reconnect", async () => {
    const sendRestart = vi.fn().mockResolvedValue({ kind: "accepted" });
    let handlers: RestartStreamHandlers | undefined;
    const makeDeps = (onPhase: RestartDeps["onPhase"]): RestartDeps => ({
      sendRestart,
      openStream: (h) => {
        handlers = h;
        return { close: () => {} };
      },
      checkStatus: () => Promise.resolve(true),
      scheduleStatusPolls: () => () => {},
      onPhase,
    });

    render(() => (
      <AppProviders>
        <RestartControl makeDeps={makeDeps} restartEnabled={() => true} />
      </AppProviders>
    ));

    await userEvent.click(
      await screen.findByRole("button", { name: "Restart gateway" })
    );

    const dialog = await screen.findByRole("alertdialog", {
      name: "Restart the gateway?",
    });
    await userEvent.click(
      within(dialog).getByRole("button", { name: "Restart" })
    );

    await waitFor(() => {
      expect(sendRestart).toHaveBeenCalledTimes(1);
    });
    expect(screen.getByText("Restarting the gateway…")).toBeVisible();
    expect(
      screen.getByRole("button", { name: "Restart gateway" })
    ).toBeDisabled();

    handlers?.onEvent({
      type: "tool_completed",
      name: "restart",
      success: true,
    });
    handlers?.onError();
    handlers?.onOpen();

    expect(await screen.findByText("Gateway restarted.")).toBeVisible();
  });

  it.each([
    "timeout",
    "transport",
    "http",
  ] as const)("reports %s safely and lets the operator retry", async (category) => {
    const sendRestart = vi
      .fn<() => Promise<RestartSendResult>>()
      .mockResolvedValueOnce({ kind: "failed", category })
      .mockResolvedValue({ kind: "accepted" });
    const report = vi.spyOn(console, "error").mockImplementation(() => {});
    let handlers: RestartStreamHandlers | undefined;
    const makeDeps = (onPhase: RestartDeps["onPhase"]): RestartDeps => ({
      sendRestart,
      onPhase,
      openStream: (next) => {
        handlers = next;
        return { close: () => {} };
      },
      checkStatus: () => Promise.resolve(true),
      scheduleStatusPolls: () => () => {},
    });
    render(() => (
      <AppProviders>
        <RestartControl makeDeps={makeDeps} restartEnabled={() => true} />
      </AppProviders>
    ));
    await userEvent.click(
      await screen.findByRole("button", { name: "Restart gateway" })
    );
    await screen.findByRole("alertdialog");
    await userEvent.click(screen.getByRole("button", { name: /^Restart$/ }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The restart request failed. Try again."
    );
    const retry = screen.getByRole("button", { name: "Retry restart" });
    expect(retry).toBeEnabled();
    expect(report).toHaveBeenCalledWith("[gateway-restart]", {
      event: "restart_failed",
      category,
    });
    await waitFor(() => {
      expect(globalThis.getComputedStyle(document.body).pointerEvents).not.toBe(
        "none"
      );
    });
    await userEvent.click(retry);
    await screen.findByRole("alertdialog");
    await userEvent.click(screen.getByRole("button", { name: /^Restart$/ }));
    await waitFor(() => expect(sendRestart).toHaveBeenCalledTimes(2));
    handlers?.onEvent({
      type: "tool_completed",
      name: "restart",
      success: true,
    });
    handlers?.onError();
    handlers?.onOpen();
    expect(await screen.findByText("Gateway restarted.")).toBeVisible();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("reports only bounded transport categories in its diagnostic", async () => {
    const report = vi.spyOn(console, "error").mockImplementation(() => {});
    render(() => (
      <AppProviders>
        <RestartControl makeDeps={rawFailureDeps} restartEnabled={() => true} />
      </AppProviders>
    ));
    await userEvent.click(
      await screen.findByRole("button", { name: "Restart gateway" })
    );
    await screen.findByRole("alertdialog");
    await userEvent.click(screen.getByRole("button", { name: /^Restart$/ }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The restart request failed. Try again."
    );
    expect(report).toHaveBeenCalledWith("[gateway-restart]", {
      event: "restart_failed",
      category: "transport",
    });
    expect(screen.getByRole("button", { name: "Retry restart" })).toBeEnabled();
  });

  it.each([
    "timeout",
    "transport",
    "http",
  ] as const)("classifies a real browser adapter %s failure without raw details", async (category) => {
    vi.spyOn(chatApi, "sendMessage").mockRejectedValue(
      new RequestError(category, "secret-response/token", 503)
    );
    const report = vi.spyOn(console, "error").mockImplementation(() => {});
    render(() => (
      <AppProviders>
        <RestartControl restartEnabled={() => true} />
      </AppProviders>
    ));
    await userEvent.click(
      await screen.findByRole("button", { name: "Restart gateway" })
    );
    await screen.findByRole("alertdialog");
    await userEvent.click(screen.getByRole("button", { name: /^Restart$/ }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The restart request failed. Try again."
    );
    expect(report).toHaveBeenCalledWith("[gateway-restart]", {
      event: "restart_failed",
      category,
    });
    const reportText = JSON.stringify(report.mock.calls);
    expect(reportText).not.toContain("secret-response");
    expect(reportText).not.toContain("503");
    expect(screen.getByRole("button", { name: "Retry restart" })).toBeEnabled();
  });

  it("ignores malformed and application-error frames but completes after transport recovery", async () => {
    vi.spyOn(chatApi, "sendMessage").mockResolvedValue({
      message_id: "restart-fixture",
      status: "accepted",
    });
    vi.stubGlobal("EventSource", GatewayEventSource);
    render(() => (
      <AppProviders>
        <RestartControl restartEnabled={() => true} />
      </AppProviders>
    ));
    await userEvent.click(
      await screen.findByRole("button", { name: "Restart gateway" })
    );
    await screen.findByRole("alertdialog");
    await userEvent.click(screen.getByRole("button", { name: /^Restart$/ }));
    await waitFor(() => expect(chatApi.sendMessage).toHaveBeenCalled());
    await waitFor(() => expect(GatewayEventSource.latest).toBeDefined());
    const source = GatewayEventSource.latest;
    if (!source) {
      throw new Error("Restart stream was not opened");
    }
    source.emit("tool_completed", {
      type: "tool_completed",
      name: "restart",
      success: true,
    });
    source.frame("response", "{ malformed json");
    source.emit("error", {
      type: "error",
      message: "Gateway reported an error",
    });
    source.open();
    expect(screen.queryByText("Gateway restarted.")).toBeNull();
    expect(screen.getByText("Restarting the gateway…")).toBeVisible();
    source.fail();
    source.open();
    expect(await screen.findByText("Gateway restarted.")).toBeVisible();
    expect(source.close).toHaveBeenCalledTimes(1);
  });
});
