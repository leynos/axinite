import { render, screen, waitFor } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { AppProviders } from "@/app/providers";
import { RestartControl } from "@/components/restart-control";
import { DEFAULT_LOCALE } from "@/lib/i18n/supported-locales";
import type { RestartDeps, RestartStreamHandlers } from "@/lib/restart";
import { setupI18nTestHarness } from "./support/i18n-test-runtime";

beforeAll(async () => {
  await setupI18nTestHarness();
});

beforeEach(async () => {
  window.localStorage.clear();
  const runtime = await import("@/lib/i18n/runtime");
  await runtime.default.changeLanguage(DEFAULT_LOCALE);
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
});
