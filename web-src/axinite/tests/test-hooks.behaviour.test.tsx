/**
 * @file Behaviour tests for the `installTestHooks` hooks in `lib/test-hooks`,
 * exercised against the app shell and chat route with a controllable gateway.
 */

import { render, screen, waitFor } from "@solidjs/testing-library";
import { beforeAll, beforeEach, describe, expect, it } from "vitest";

import { AppProviders } from "@/app/providers";
import { ShellChrome } from "@/components/app-shell";
import { ChatPreview } from "@/components/chat-preview";
import { DEFAULT_LOCALE } from "@/lib/i18n/supported-locales";
import { installTestHooks } from "@/lib/test-hooks";
import {
  GatewayEventSource,
  GatewayTestServer,
} from "./support/gateway-test-server";
import { setupI18nTestHarness } from "./support/i18n-test-runtime";

beforeAll(async () => {
  await setupI18nTestHarness();
});

beforeEach(async () => {
  new GatewayTestServer()
    .respond("GET", "/api/chat/threads", {
      assistant_thread: {
        id: "thread-1",
        state: "Idle",
        turn_count: 0,
        created_at: "2026-03-26T12:00:00Z",
        updated_at: "2026-03-26T12:00:00Z",
        title: "Chat thread",
      },
      threads: [],
      active_thread: "thread-1",
    })
    .respond("GET", "/api/chat/history", {
      thread_id: "thread-1",
      turns: [],
      has_more: false,
    });

  globalThis.localStorage.clear();
  document.documentElement.lang = "";
  document.documentElement.dir = "";
  const runtime = await import("@/lib/i18n/runtime");
  await runtime.default.changeLanguage(DEFAULT_LOCALE);
});

const renderShellWithChat = () =>
  render(() => (
    <AppProviders>
      <ShellChrome activePath="/chat" usePlainLinks>
        <ChatPreview />
      </ShellChrome>
    </AppProviders>
  ));

// Install the hooks, mount the chat surface, wait for it to register its stream
// controls, then drive the fake stream to the "connected" state. Returns the
// hook surface and the connection indicator so each test can act from a known,
// connected baseline. Keeping the shared waits here holds the per-test bodies
// below the complex-method threshold.
async function setupConnectedChat(): Promise<{
  hooks: typeof globalThis.__axinite;
  indicator: HTMLElement;
}> {
  installTestHooks();
  renderShellWithChat();

  await waitFor(() => expect(GatewayEventSource.latest).toBeDefined());

  const hooks = globalThis.__axinite;
  const indicator = screen.getByTestId("sse-status");

  // The stream is opening; simulate the browser firing `open`.
  GatewayEventSource.latest?.open();
  await waitFor(() => {
    expect(indicator).toHaveAttribute("data-state", "connected");
  });

  return { hooks, indicator };
}

async function expectIndicatorState(
  indicator: HTMLElement,
  state: string
): Promise<void> {
  await waitFor(() => {
    expect(indicator).toHaveAttribute("data-state", state);
  });
}

describe("window.__axinite test hooks", () => {
  it("exposes the versioned hook surface and drives chat updates via emitChatEvent", async () => {
    const { hooks } = await setupConnectedChat();

    expect(hooks?.version).toBe(1);

    // emitChatEvent feeds a synthetic event into the same handler the stream
    // uses; a job_started event renders a visible job card.
    hooks?.emitChatEvent({
      type: "job_started",
      job_id: "abcdef1234567890",
      title: "Hooked preview job",
      browse_url: "https://example.test/projects/9/",
    });
    expect(await screen.findByText("Hooked preview job")).toBeVisible();
  });

  it("closeChatStream flips the connection indicator to disconnected", async () => {
    const { hooks, indicator } = await setupConnectedChat();

    hooks?.closeChatStream();
    await expectIndicatorState(indicator, "disconnected");
    expect(GatewayEventSource.latest?.readyState).toBe(2);
  });

  it("reconnectChatStream re-opens the stream and returns to connected", async () => {
    const { hooks, indicator } = await setupConnectedChat();

    hooks?.closeChatStream();
    await expectIndicatorState(indicator, "disconnected");

    // reconnectChatStream re-opens the stream (re-registering listeners); the
    // indicator returns to connected once the fake stream fires `open`.
    hooks?.reconnectChatStream();
    GatewayEventSource.latest?.open();
    await expectIndicatorState(indicator, "connected");
  });
});
