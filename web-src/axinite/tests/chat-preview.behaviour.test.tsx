import { render, screen, waitFor } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { AppProviders } from "@/app/providers";
import { ChatPreview } from "@/components/chat-preview";
import { DEFAULT_LOCALE } from "@/lib/i18n/supported-locales";
import { setupI18nTestHarness } from "./support/i18n-test-runtime";

const chatApiMocks = vi.hoisted(() => ({
  createThread: vi.fn(),
  fetchHistory: vi.fn(),
  fetchThreads: vi.fn(),
  sendMessage: vi.fn(),
  submitApproval: vi.fn(),
  connectChatEvents: vi.fn(),
  listener: null as
    | ((event: { type: string; content?: string; thread_id?: string }) => void)
    | null,
}));

vi.mock("@/lib/api/chat", () => ({
  connectChatEvents: chatApiMocks.connectChatEvents,
  createThread: chatApiMocks.createThread,
  fetchHistory: chatApiMocks.fetchHistory,
  fetchThreads: chatApiMocks.fetchThreads,
  sendMessage: chatApiMocks.sendMessage,
  submitApproval: chatApiMocks.submitApproval,
}));

beforeAll(async () => {
  await setupI18nTestHarness();
});

beforeEach(async () => {
  chatApiMocks.listener = null;
  chatApiMocks.createThread.mockReset();
  chatApiMocks.fetchThreads.mockReset();
  chatApiMocks.fetchHistory.mockReset();
  chatApiMocks.sendMessage.mockReset();
  chatApiMocks.submitApproval.mockReset();
  chatApiMocks.connectChatEvents.mockReset();

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
  chatApiMocks.sendMessage.mockResolvedValue({
    message_id: "message-1",
    status: "queued",
  });
  chatApiMocks.connectChatEvents.mockImplementation(
    (
      listener: (event: {
        type: string;
        content?: string;
        thread_id?: string;
      }) => void
    ) => {
      chatApiMocks.listener = listener;
      return {
        close: vi.fn(),
      };
    }
  );
});

describe("chat preview behaviour", () => {
  it("shows an optimistic user turn and spinner while awaiting a response", async () => {
    const { container } = render(() => (
      <AppProviders>
        <ChatPreview />
      </AppProviders>
    ));

    const composer = await screen.findByLabelText("Message composer");
    await userEvent.type(composer, "Check the deployment status");
    await userEvent.click(screen.getByRole("button", { name: "Send" }));

    await waitFor(() => {
      expect(chatApiMocks.sendMessage).toHaveBeenCalledWith(
        expect.objectContaining({
          content: "Check the deployment status",
          thread_id: "thread-1",
        })
      );
    });

    expect(screen.getByText("Check the deployment status")).toBeVisible();
    expect(container.querySelector('[aria-busy="true"]')).not.toBeNull();

    chatApiMocks.listener?.({
      type: "stream_chunk",
      content: "Reviewing the deployment now.",
      thread_id: "thread-1",
    });

    await waitFor(() => {
      expect(screen.getByText("Reviewing the deployment now.")).toBeVisible();
    });
    expect(container.querySelector('[aria-busy="true"]')).toBeNull();
  });
});
