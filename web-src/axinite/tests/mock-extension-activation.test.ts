/** @file Extension activation preserves OAuth and manual-token response contracts. */
import { describe, expect, it } from "vitest";
import type { ChatSseEvent } from "@/lib/api/contracts";
import { MockBackendState } from "../../mock-backend/src/state";

describe("mock extension activation", () => {
  it.each([
    {
      name: "google-drive",
      setupUrl: "https://example.invalid/token-help",
      authUrl: "https://oauth.example.invalid/consent",
    },
    {
      name: "slack",
      setupUrl: "/api/extensions/slack/setup",
      authUrl: undefined,
    },
  ])("keeps authentication details for $name", ({
    name,
    setupUrl,
    authUrl,
  }) => {
    const state = new MockBackendState();
    if (name === "slack") state.installExtension(name);
    const events: ChatSseEvent[] = [];
    const unsubscribe = state.subscribeToChat({
      send: (event) => events.push(event),
      close: () => {},
    });
    const result = state.activateExtension(name);
    expect(result).toMatchObject({ awaiting_token: true, activated: false });
    const event = events.find((entry) => entry.type === "auth_required");
    expect(event).toMatchObject({
      type: "auth_required",
      extension_name: name,
      setup_url: setupUrl,
    });
    if (authUrl) {
      expect(event).toHaveProperty("auth_url", authUrl);
      expect(result).toHaveProperty("auth_url", authUrl);
    } else {
      expect(event).not.toHaveProperty("auth_url");
      expect(result).not.toHaveProperty("auth_url");
    }
    expect(
      state.listExtensions().extensions.find((entry) => entry.name === name)
        ?.active
    ).toBe(false);
    unsubscribe();
  });

  it("activates an authenticated extension and publishes its status", () => {
    const state = new MockBackendState();
    state.installExtension("slack");
    state.submitExtensionSetup("slack", { token: "valid-token" });
    const events: ChatSseEvent[] = [];
    const unsubscribe = state.subscribeToChat({
      send: (event) => events.push(event),
      close: () => {},
    });
    expect(state.activateExtension("slack")).toMatchObject({ activated: true });
    expect(events).toContainEqual(
      expect.objectContaining({
        type: "extension_status",
        extension_name: "slack",
        status: "active",
      })
    );
    unsubscribe();
  });

  it("rejects an unknown extension", () => {
    expect(() => new MockBackendState().activateExtension("missing")).toThrow(
      "Extension not found"
    );
  });
});
