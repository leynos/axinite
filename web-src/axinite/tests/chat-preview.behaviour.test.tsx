import { render, screen, waitFor } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { AppProviders } from "@/app/providers";
import { ChatPreview } from "@/components/chat-preview";
import { DEFAULT_LOCALE } from "@/lib/i18n/supported-locales";
import { setupI18nTestHarness } from "./support/i18n-test-runtime";

type ChatEvent = { type: string; [key: string]: unknown };

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
  chatApiMocks.createThread.mockReset();
  chatApiMocks.fetchThreads.mockReset();
  chatApiMocks.fetchHistory.mockReset();
  chatApiMocks.sendMessage.mockReset();
  chatApiMocks.submitApproval.mockReset();
  chatApiMocks.submitAuthToken.mockReset();
  chatApiMocks.cancelAuth.mockReset();
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
    (listener: (event: ChatEvent) => void) => {
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

  const renderPreview = () =>
    render(() => (
      <AppProviders>
        <ChatPreview />
      </AppProviders>
    ));

  const waitForListener = async () => {
    await waitFor(() => {
      expect(chatApiMocks.listener).not.toBeNull();
    });
  };

  it("renders a generated-image card from an image_generated event", async () => {
    renderPreview();
    await waitForListener();

    chatApiMocks.listener?.({
      type: "image_generated",
      data_url: "data:image/png;base64,AAAA",
      path: "workspace/generated/preview.png",
      thread_id: "thread-1",
    });

    const image = await screen.findByAltText("Generated image");
    expect(image).toHaveAttribute("src", "data:image/png;base64,AAAA");
    expect(screen.getByText("workspace/generated/preview.png")).toBeVisible();
  });

  it("renders a job card with a link from a job_started event", async () => {
    renderPreview();
    await waitForListener();

    chatApiMocks.listener?.({
      type: "job_started",
      job_id: "abcdef1234567890",
      title: "Spawned preview job",
      browse_url: "https://example.test/projects/1/",
    });

    expect(await screen.findByText("Spawned preview job")).toBeVisible();
    expect(screen.getByText("Job abcdef12")).toBeVisible();
    const openLink = screen.getByRole("link", { name: "Open in Jobs" });
    expect(openLink.getAttribute("href")).toContain("jobs");
    const browseLink = screen.getByRole("link", { name: "Browse" });
    expect(browseLink).toHaveAttribute(
      "href",
      "https://example.test/projects/1/"
    );
    expect(browseLink).toHaveAttribute("target", "_blank");
  });

  it("submits an auth token and dismisses the card on success", async () => {
    chatApiMocks.submitAuthToken.mockResolvedValue({
      success: true,
      message: "Authentication completed.",
    });
    renderPreview();
    await waitForListener();

    chatApiMocks.listener?.({
      type: "auth_required",
      extension_name: "github",
      auth_url: "https://auth.example.test/oauth",
    });

    expect(
      await screen.findByText("Authentication required for github")
    ).toBeVisible();

    const tokenInput = screen.getByLabelText("Access token");
    await userEvent.type(tokenInput, "valid-token");
    await userEvent.click(screen.getByRole("button", { name: "Submit" }));

    await waitFor(() => {
      expect(chatApiMocks.submitAuthToken).toHaveBeenCalledWith({
        extension_name: "github",
        token: "valid-token",
      });
    });
    await waitFor(() => {
      expect(
        screen.queryByText("Authentication required for github")
      ).toBeNull();
    });
    expect(screen.getByText("Authentication completed.")).toBeVisible();
  });

  it("keeps the auth card and shows an error when the token is rejected", async () => {
    chatApiMocks.submitAuthToken.mockResolvedValue({
      success: false,
      message: "Invalid token.",
    });
    renderPreview();
    await waitForListener();

    chatApiMocks.listener?.({
      type: "auth_required",
      extension_name: "github",
      auth_url: "https://auth.example.test/oauth",
    });

    await screen.findByText("Authentication required for github");
    await userEvent.type(screen.getByLabelText("Access token"), "short");
    await userEvent.click(screen.getByRole("button", { name: "Submit" }));

    await waitFor(() => {
      expect(
        screen.getByText(
          "Authentication failed. Check the token and try again."
        )
      ).toBeVisible();
    });
    expect(
      screen.getByText("Authentication required for github")
    ).toBeVisible();
  });

  it("cancels the auth flow via the cancel endpoint", async () => {
    chatApiMocks.cancelAuth.mockResolvedValue({
      success: true,
      message: "Auth cancelled.",
    });
    renderPreview();
    await waitForListener();

    chatApiMocks.listener?.({
      type: "auth_required",
      extension_name: "github",
    });

    await screen.findByText("Authentication required for github");
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));

    await waitFor(() => {
      expect(chatApiMocks.cancelAuth).toHaveBeenCalledWith({
        extension_name: "github",
      });
    });
    await waitFor(() => {
      expect(
        screen.queryByText("Authentication required for github")
      ).toBeNull();
    });
  });

  it("removes the auth card when auth_completed arrives", async () => {
    renderPreview();
    await waitForListener();

    chatApiMocks.listener?.({
      type: "auth_required",
      extension_name: "github",
    });
    await screen.findByText("Authentication required for github");

    chatApiMocks.listener?.({
      type: "auth_completed",
      extension_name: "github",
      success: true,
      message: "All set.",
    });

    await waitFor(() => {
      expect(
        screen.queryByText("Authentication required for github")
      ).toBeNull();
    });
    expect(screen.getByText("All set.")).toBeVisible();
  });

  it("rejects an oversize image with a notice and stages a valid one", async () => {
    const { container } = renderPreview();
    await screen.findByLabelText("Message composer");

    const fileInput = container.querySelector(
      'input[type="file"]'
    ) as HTMLInputElement;

    const oversize = new File(
      [new Uint8Array(5 * 1024 * 1024 + 1)],
      "huge.png",
      { type: "image/png" }
    );
    await userEvent.upload(fileInput, oversize);

    expect(
      await screen.findByText("huge.png is larger than the 5 MB limit.")
    ).toBeVisible();

    const valid = new File([new Uint8Array([1, 2, 3, 4])], "ok.png", {
      type: "image/png",
    });
    await userEvent.upload(fileInput, valid);

    const thumb = await screen.findByAltText("ok.png");
    expect(thumb).toBeVisible();

    await userEvent.type(
      screen.getByLabelText("Message composer"),
      "Here is an image"
    );
    await userEvent.click(screen.getByRole("button", { name: "Send" }));

    await waitFor(() => {
      expect(chatApiMocks.sendMessage).toHaveBeenCalled();
    });
    const payload = chatApiMocks.sendMessage.mock.calls.at(-1)?.[0];
    expect(payload.images).toHaveLength(1);
    expect(payload.images[0].media_type).toBe("image/png");
    expect(typeof payload.images[0].data).toBe("string");
    expect(payload.images[0].data.length).toBeGreaterThan(0);
  });
});
