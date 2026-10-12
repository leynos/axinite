/**
 * @file Behaviour tests for `RestartControl`: whether it is shown, and how it
 * reflects the restart phases reported by the injected restart dependencies.
 */

import { render, screen, waitFor } from "@solidjs/testing-library";
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
import type { ChatSseEvent } from "@/lib/api/contracts";
import { DEFAULT_LOCALE } from "@/lib/i18n/supported-locales";
import {
  RestartCommandError,
  type RestartDeps,
  type RestartStreamHandlers,
} from "@/lib/restart";
import { setupI18nTestHarness } from "./support/i18n-test-runtime";

const dialogStyles = document.createElement("style");

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
  window.localStorage.clear();
  const runtime = await import("@/lib/i18n/runtime");
  await runtime.default.changeLanguage(DEFAULT_LOCALE);
});

afterEach(() => vi.restoreAllMocks());

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
    const sendRestart = vi.fn().mockResolvedValue({
      message_id: "m1",
      status: "queued",
    });
    let handlers: RestartStreamHandlers | undefined;
    const makeDeps = (onPhase: RestartDeps["onPhase"]): RestartDeps => ({
      sendRestart,
      openStream: (h) => {
        handlers = h;
        return { close: () => undefined };
      },
      checkStatus: () => Promise.resolve(true),
      scheduleStatusPolls: () => () => undefined,
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
      screen.getAllByRole("button", { name: "Restart" }).at(-1) as HTMLElement
    );
    void dialog;

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
      .fn()
      .mockRejectedValueOnce(new RestartCommandError(category))
      .mockResolvedValue({ status: "accepted" });
    const report = vi
      .spyOn(console, "error")
      .mockImplementation(() => undefined);
    let handlers: RestartStreamHandlers | undefined;
    const makeDeps = (onPhase: RestartDeps["onPhase"]): RestartDeps => ({
      sendRestart,
      onPhase,
      openStream: (next) => {
        handlers = next;
        return { close: () => undefined };
      },
      checkStatus: () => Promise.resolve(true),
      scheduleStatusPolls: () => () => undefined,
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
      expect(window.getComputedStyle(document.body).pointerEvents).not.toBe(
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

  it("never exposes a raw rejected error in its diagnostic or operator message", async () => {
    const report = vi
      .spyOn(console, "error")
      .mockImplementation(() => undefined);
    const makeDeps = (onPhase: RestartDeps["onPhase"]): RestartDeps => ({
      sendRestart: () => Promise.reject(new Error("secret-token/private-url")),
      onPhase,
      openStream: () => ({ close: () => undefined }),
      checkStatus: () => Promise.resolve(true),
      scheduleStatusPolls: () => () => undefined,
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
    expect(report).toHaveBeenCalledWith("[gateway-restart]", {
      event: "restart_failed",
      category: "transport",
    });
    expect(JSON.stringify(report.mock.calls)).not.toContain("secret-token");
    expect(document.body.textContent).not.toContain("private-url");
  });

  it.each([
    "timeout",
    "transport",
    "http",
  ] as const)("classifies a real browser adapter %s failure without raw details", async (category) => {
    vi.spyOn(chatApi, "sendMessage").mockRejectedValue(
      new RequestError(category, "secret-response/token", 503)
    );
    const report = vi
      .spyOn(console, "error")
      .mockImplementation(() => undefined);
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
    expect(JSON.stringify(report.mock.calls)).not.toContain("secret-response");
    expect(JSON.stringify(report.mock.calls)).not.toContain("503");
    expect(screen.getByRole("button", { name: "Retry restart" })).toBeEnabled();
  });

  it("ignores malformed and application-error frames but completes after transport recovery", async () => {
    vi.spyOn(chatApi, "sendMessage").mockResolvedValue({
      message_id: "restart-fixture",
      status: "accepted",
    });
    let emit: ((event: ChatSseEvent) => void) | undefined;
    let malformedFrame: () => void = () => undefined;
    const source = {
      onopen: null,
      onerror: null,
      close: vi.fn(),
    } as unknown as EventSource;
    const connect = vi
      .spyOn(chatApi, "connectChatEvents")
      .mockImplementation((listener, parseError) => {
        emit = listener;
        malformedFrame = () => {
          parseError?.();
        };
        return source;
      });
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
    expect(connect.mock.calls[0]).toHaveLength(1);
    emit?.({ type: "tool_completed", name: "restart", success: true });
    malformedFrame();
    source.onerror?.(
      new MessageEvent("error", {
        data: JSON.stringify({
          type: "error",
          message: "Gateway reported an error",
        }),
      })
    );
    source.onopen?.(new Event("open"));
    expect(screen.queryByText("Gateway restarted.")).toBeNull();
    expect(screen.getByText("Restarting the gateway…")).toBeVisible();
    source.onerror?.(new Event("error"));
    source.onopen?.(new Event("open"));
    expect(await screen.findByText("Gateway restarted.")).toBeVisible();
    expect(source.close).toHaveBeenCalledTimes(1);
  });
});
