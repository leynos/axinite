/**
 * @file Behaviour tests for the chat route (`ChatPreview`) using the real API
 * modules and controlled in-process gateway responses and SSE frames.
 */

import { render, screen, waitFor } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { beforeAll, beforeEach, describe, expect, it } from "vitest";

import { AppProviders } from "@/app/providers";
import { ChatPreview } from "@/components/chat-preview";
import { DEFAULT_LOCALE } from "@/lib/i18n/supported-locales";
import {
  GatewayEventSource,
  GatewayTestServer,
} from "./support/gateway-test-server";
import { setupI18nTestHarness } from "./support/i18n-test-runtime";
import { TestProviders } from "./support/test-providers";

let gateway: GatewayTestServer;

function imageFile(name: string): File {
  return new File([new Uint8Array([1, 2, 3, 4])], name, {
    type: "image/png",
  });
}

function fileInput(container: HTMLElement): HTMLInputElement {
  const input = container.querySelector<HTMLInputElement>('input[type="file"]');
  if (!input) {
    throw new Error("Chat image input was not rendered");
  }
  return input;
}

function renderPreview() {
  return render(() => (
    <AppProviders>
      <ChatPreview />
    </AppProviders>
  ));
}

async function waitForEventSource(): Promise<void> {
  await waitFor(() => {
    expect(GatewayEventSource.latest).toBeDefined();
  });
}

class DeferredResponse {
  readonly promise: Promise<Response>;
  private resolveResponse: ((response: Response) => void) | undefined;

  constructor() {
    this.promise = new Promise<Response>((resolve) => {
      this.resolveResponse = resolve;
    });
  }

  resolve(response: Response): void {
    if (!this.resolveResponse) {
      throw new Error("Deferred response was not initialized");
    }
    this.resolveResponse(response);
  }
}

function deferredResponse(): DeferredResponse {
  return new DeferredResponse();
}

beforeAll(async () => {
  await setupI18nTestHarness();
});

beforeEach(async () => {
  gateway = new GatewayTestServer()
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
    })
    .respond("POST", "/api/chat/send", {
      message_id: "message-1",
      status: "queued",
    })
    .respond("POST", "/api/chat/auth-token", {
      success: true,
      message: "Authentication completed.",
    })
    .respond("POST", "/api/chat/auth-cancel", {
      success: true,
      message: "Auth cancelled.",
    })
    .respond("POST", "/api/chat/approval", { success: true, message: "ok" });

  globalThis.localStorage.clear();
  document.documentElement.lang = "";
  document.documentElement.dir = "";
  const runtime = await import("@/lib/i18n/runtime");
  await runtime.default.changeLanguage(DEFAULT_LOCALE);
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
      expect(
        gateway.requests.some((request) => {
          const body = request.body;
          return (
            request.path === "/api/chat/send" &&
            body?.["content"] === "Check the deployment status" &&
            body["thread_id"] === "thread-1"
          );
        })
      ).toBe(true);
    });

    expect(screen.getByText("Check the deployment status")).toBeVisible();
    expect(container.querySelector('[aria-busy="true"]')).not.toBeNull();

    GatewayEventSource.latest?.emit("stream_chunk", {
      type: "stream_chunk",
      content: "Reviewing the deployment now.",
      thread_id: "thread-1",
    });

    await waitFor(() => {
      expect(screen.getByText("Reviewing the deployment now.")).toBeVisible();
    });
    expect(container.querySelector('[aria-busy="true"]')).toBeNull();
  });

  it("renders a generated-image card from an image_generated event", async () => {
    renderPreview();
    await waitForEventSource();

    GatewayEventSource.latest?.emit("image_generated", {
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
    await waitForEventSource();

    GatewayEventSource.latest?.emit("job_started", {
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
    renderPreview();
    await waitForEventSource();

    GatewayEventSource.latest?.emit("auth_required", {
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
      expect(
        gateway.requests.some((request) => {
          const body = request.body;
          return (
            request.path === "/api/chat/auth-token" &&
            body?.["extension_name"] === "github" &&
            body["token"] === "valid-token"
          );
        })
      ).toBe(true);
    });
    await waitFor(() => {
      expect(
        screen.queryByText("Authentication required for github")
      ).toBeNull();
    });
    expect(screen.getByText("Authentication completed.")).toBeVisible();
  });

  it("keeps the auth card and shows an error when the token is rejected", async () => {
    gateway.respond("POST", "/api/chat/auth-token", {
      success: false,
      message: "Invalid token.",
    });
    renderPreview();
    await waitForEventSource();

    GatewayEventSource.latest?.emit("auth_required", {
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
    renderPreview();
    await waitForEventSource();

    GatewayEventSource.latest?.emit("auth_required", {
      type: "auth_required",
      extension_name: "github",
    });

    await screen.findByText("Authentication required for github");
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));

    await waitFor(() => {
      expect(
        gateway.requests.some(
          (request) =>
            request.path === "/api/chat/auth-cancel" &&
            request.body?.["extension_name"] === "github"
        )
      ).toBe(true);
    });
    await waitFor(() => {
      expect(
        screen.queryByText("Authentication required for github")
      ).toBeNull();
    });
  });

  it("removes the auth card when auth_completed arrives", async () => {
    renderPreview();
    await waitForEventSource();

    GatewayEventSource.latest?.emit("auth_required", {
      type: "auth_required",
      extension_name: "github",
    });
    await screen.findByText("Authentication required for github");

    GatewayEventSource.latest?.emit("auth_completed", {
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
    const input = fileInput(container);

    const oversize = new File(
      [new Uint8Array(5 * 1024 * 1024 + 1)],
      "huge.png",
      { type: "image/png" }
    );
    await userEvent.upload(input, oversize);

    expect(
      await screen.findByText("huge.png is larger than the 5 MB limit.")
    ).toBeVisible();

    const valid = new File([new Uint8Array([1, 2, 3, 4])], "ok.png", {
      type: "image/png",
    });
    await userEvent.upload(input, valid);

    const thumb = await screen.findByAltText("ok.png");
    expect(thumb).toBeVisible();

    await userEvent.type(
      screen.getByLabelText("Message composer"),
      "Here is an image"
    );
    await userEvent.click(screen.getByRole("button", { name: "Send" }));

    await waitFor(() => {
      expect(
        gateway.requests.some((request) => request.path === "/api/chat/send")
      ).toBe(true);
    });
    const sentMessage = gateway.requests.find(
      (request) => request.path === "/api/chat/send"
    );
    expect(sentMessage?.body?.["images"]).toMatchObject([
      {
        media_type: "image/png",
        data: expect.stringMatching(/.+/u),
      },
    ]);
  });

  it("keeps images staged while a send is in flight", async () => {
    const pendingSend = deferredResponse();
    gateway.route("POST", "/api/chat/send", () => pendingSend.promise);
    const { container } = renderPreview();
    await screen.findByLabelText("Message composer");
    const input = fileInput(container);

    await userEvent.upload(input, imageFile("first.png"));
    await screen.findByAltText("first.png");
    await userEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => {
      expect(
        gateway.requests.filter((request) => request.path === "/api/chat/send")
      ).toHaveLength(1);
    });

    // Staged after Send, before the gateway answers.
    await userEvent.upload(input, imageFile("second.png"));
    await screen.findByAltText("second.png");
    pendingSend.resolve(
      Response.json({ message_id: "message-1", status: "queued" })
    );

    await waitFor(() => {
      expect(screen.queryByAltText("first.png")).toBeNull();
    });
    expect(screen.getByAltText("second.png")).toBeVisible();
    const send = gateway.requests.find(
      (request) => request.path === "/api/chat/send"
    );
    expect(send?.body?.["images"]).toHaveLength(1);
  });

  it("tags user and assistant turns with data-role markers", async () => {
    gateway.respond("GET", "/api/chat/history", {
      thread_id: "thread-1",
      turns: [
        {
          turn_number: 1,
          user_input: "Ping the daemon",
          response: "Pong from the daemon.",
          state: "Complete",
          started_at: "2026-03-26T12:00:00Z",
          completed_at: "2026-03-26T12:00:01Z",
          tool_calls: [],
        },
      ],
      has_more: false,
    });

    // TestProviders builds a fresh QueryClient so the non-empty history is not
    // shadowed by the empty "thread-1" history cached by earlier cases in the
    // module-singleton client that AppProviders shares.
    const { container } = render(() => (
      <TestProviders>
        <ChatPreview />
      </TestProviders>
    ));

    await screen.findByText("Ping the daemon");
    expect(container.querySelector('[data-role="user"]')).not.toBeNull();
    await waitFor(() => {
      expect(container.querySelector('[data-role="assistant"]')).not.toBeNull();
    });
  });

  it("offers Approve/Always/Deny on the pending-approval card and disables them while submitting", async () => {
    gateway.respond("GET", "/api/chat/history", {
      thread_id: "thread-1",
      turns: [],
      has_more: false,
      pending_approval: {
        request_id: "req-42",
        tool_name: "shell",
        description: "Run a shell command",
        parameters: '{ "cmd": "ls" }',
      },
    });

    let resolveApproval: (() => void) | undefined;
    gateway.route(
      "POST",
      "/api/chat/approval",
      () =>
        new Promise((resolve) => {
          resolveApproval = () =>
            resolve(Response.json({ success: true, message: "ok" }));
        })
    );

    // Fresh QueryClient (see the data-role test) so the pending_approval
    // history is not masked by the cached empty "thread-1" history.
    const { container } = render(() => (
      <TestProviders>
        <ChatPreview />
      </TestProviders>
    ));

    const approveButton = await screen.findByRole("button", {
      name: "Approve",
    });
    const alwaysButton = screen.getByRole("button", { name: "Always" });
    const denyButton = screen.getByRole("button", { name: "Deny" });

    // The card carries the request id for the e2e suite to target.
    expect(
      container.querySelector('[data-request-id="req-42"]')
    ).not.toBeNull();

    await userEvent.click(alwaysButton);

    await waitFor(() => {
      expect(
        gateway.requests.some(
          (request) =>
            request.path === "/api/chat/approval" &&
            request.body?.["request_id"] === "req-42" &&
            request.body["action"] === "always"
        )
      ).toBe(true);
    });

    // All three actions are disabled while the mutation is pending.
    await waitFor(() => {
      expect(approveButton).toBeDisabled();
    });
    expect(alwaysButton).toBeDisabled();
    expect(denyButton).toBeDisabled();

    resolveApproval?.();
  });
});
