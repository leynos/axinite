import { render, screen, waitFor } from "@solidjs/testing-library";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { AppProviders } from "@/app/providers";
import { ShellChrome } from "@/components/app-shell";
import { ChatPreview } from "@/components/chat-preview";
import { DEFAULT_LOCALE } from "@/lib/i18n/supported-locales";
import { installTestHooks } from "@/lib/test-hooks";
import { setupI18nTestHarness } from "./support/i18n-test-runtime";

type ChatEvent = { type: string; [key: string]: unknown };

type FakeSource = {
  onopen: (() => void) | null;
  onerror: (() => void) | null;
  close: ReturnType<typeof vi.fn>;
};

const chatApiMocks = vi.hoisted(() => ({
  createThread: vi.fn(),
  fetchHistory: vi.fn(),
  fetchThreads: vi.fn(),
  sendMessage: vi.fn(),
  submitApproval: vi.fn(),
  submitAuthToken: vi.fn(),
  cancelAuth: vi.fn(),
  connectChatEvents: vi.fn(),
  listener: null as ((event: ChatEvent) => void) | null,
  source: null as FakeSource | null,
}));

vi.mock("@/lib/api/chat", () => ({
  connectChatEvents: chatApiMocks.connectChatEvents,
  createThread: chatApiMocks.createThread,
  fetchHistory: chatApiMocks.fetchHistory,
  fetchThreads: chatApiMocks.fetchThreads,
  sendMessage: chatApiMocks.sendMessage,
  submitApproval: chatApiMocks.submitApproval,
  submitAuthToken: chatApiMocks.submitAuthToken,
  cancelAuth: chatApiMocks.cancelAuth,
}));

beforeAll(async () => {
  await setupI18nTestHarness();
});

beforeEach(async () => {
  chatApiMocks.listener = null;
  chatApiMocks.source = null;
  for (const mock of [
    chatApiMocks.createThread,
    chatApiMocks.fetchThreads,
    chatApiMocks.fetchHistory,
    chatApiMocks.sendMessage,
    chatApiMocks.submitApproval,
    chatApiMocks.connectChatEvents,
  ]) {
    mock.mockReset();
  }

  window.localStorage.clear();
  document.documentElement.lang = "";
  document.documentElement.dir = "";
  const runtime = await import("@/lib/i18n/runtime");
  await runtime.default.changeLanguage(DEFAULT_LOCALE);

  chatApiMocks.fetchThreads.mockResolvedValue({
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
  });
  chatApiMocks.fetchHistory.mockResolvedValue({
    thread_id: "thread-1",
    turns: [],
    has_more: false,
  });

  // A controllable fake EventSource: the chat surface assigns `onopen`, so a
  // test can drive the "stream opened" transition deterministically.
  chatApiMocks.connectChatEvents.mockImplementation(
    (listener: (event: ChatEvent) => void, onError?: () => void) => {
      chatApiMocks.listener = listener;
      const source: FakeSource = {
        onopen: null,
        onerror: onError ?? null,
        close: vi.fn(),
      };
      chatApiMocks.source = source;
      return source;
    }
  );
});

const renderShellWithChat = () =>
  render(() => (
    <AppProviders>
      <ShellChrome activePath="/chat" usePlainLinks>
        <ChatPreview />
      </ShellChrome>
    </AppProviders>
  ));

describe("window.__axinite test hooks", () => {
  it("drives chat updates and the connection indicator through the hook surface", async () => {
    installTestHooks();
    renderShellWithChat();

    // The chat surface registers its stream controls on mount.
    await waitFor(() => {
      expect(chatApiMocks.source).not.toBeNull();
    });

    const hooks = window.__axinite;
    expect(hooks?.version).toBe(1);

    const indicator = screen.getByTestId("sse-status");

    // The stream is opening; simulate the browser firing `open`.
    chatApiMocks.source?.onopen?.();
    await waitFor(() => {
      expect(indicator).toHaveAttribute("data-state", "connected");
    });

    // emitChatEvent feeds a synthetic event into the same handler the stream
    // uses; a job_started event renders a visible job card.
    hooks?.emitChatEvent({
      type: "job_started",
      job_id: "abcdef1234567890",
      title: "Hooked preview job",
      browse_url: "https://example.test/projects/9/",
    });
    expect(await screen.findByText("Hooked preview job")).toBeVisible();

    // closeChatStream flips the indicator to disconnected.
    hooks?.closeChatStream();
    await waitFor(() => {
      expect(indicator).toHaveAttribute("data-state", "disconnected");
    });
    expect(chatApiMocks.source?.close).toHaveBeenCalled();

    // reconnectChatStream re-opens the stream (re-registering listeners); the
    // indicator returns to connected once the fake stream fires `open`.
    hooks?.reconnectChatStream();
    chatApiMocks.source?.onopen?.();
    await waitFor(() => {
      expect(indicator).toHaveAttribute("data-state", "connected");
    });
  });
});
