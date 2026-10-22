/**
 * @file Behaviour tests for channel pairing on the extensions route
 * (`ExtensionsPreview`), with controlled in-process gateway responses.
 */

import { render, screen, waitFor, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { ExtensionsPreview } from "@/components/extensions-preview";
import type { ExtensionInfo, PairingListResponse } from "@/lib/api/contracts";
import { DEFAULT_LOCALE } from "@/lib/i18n/supported-locales";
import { GatewayTestServer } from "./support/gateway-test-server";
import {
  setApiRequestHandler,
  setupI18nTestHarness,
} from "./support/i18n-test-runtime";
import { TestProviders } from "./support/test-providers";

let gateway: GatewayTestServer;
let extensions: ExtensionInfo[];
let pairing: PairingListResponse;

function wasmChannel(activationStatus: string): ExtensionInfo {
  return {
    active: false,
    activation_status: activationStatus,
    authenticated: false,
    description: "WhatsApp channel bridge.",
    display_name: "WhatsApp",
    has_auth: false,
    kind: "wasm_channel",
    name: "whatsapp",
    needs_setup: false,
    tools: [],
    version: "0.1.0",
  };
}

beforeAll(async () => {
  await setupI18nTestHarness();
});

beforeEach(async () => {
  globalThis.localStorage.clear();
  document.documentElement.lang = "";
  document.documentElement.dir = "";
  const runtime = await import("@/lib/i18n/runtime");
  await runtime.default.changeLanguage(DEFAULT_LOCALE);

  extensions = [];
  pairing = {
    channel: "whatsapp",
    requests: [],
  };
  gateway = new GatewayTestServer()
    .route("GET", "/api/extensions", async () => Response.json({ extensions }))
    .respond("GET", "/api/extensions/tools", { tools: [] })
    .respond("GET", "/api/extensions/registry", { entries: [] })
    .respond("GET", "/api/extensions/whatsapp/setup", {
      kind: "wasm_channel",
      name: "whatsapp",
      secrets: [],
    })
    .route("GET", "/api/pairing/whatsapp", async () => Response.json(pairing))
    .respond("POST", "/api/pairing/whatsapp/approve", {
      success: true,
      message: "Pairing approved",
    });
});

afterEach(() => setApiRequestHandler(undefined));

describe("extension pairing and activation stepper", () => {
  it("shows the awaiting-pairing stepper state for a pairing channel", async () => {
    extensions = [wasmChannel("pairing")];

    render(() => (
      <TestProviders>
        <ExtensionsPreview />
      </TestProviders>
    ));

    const stepper = await screen.findByRole("list", {
      name: "Activation progress",
    });
    expect(within(stepper).getByText("Awaiting pairing")).toBeVisible();
    expect(within(stepper).getByText("In progress")).toBeVisible();
  });

  it("shows the failed stepper state when activation fails", async () => {
    extensions = [wasmChannel("failed")];

    render(() => (
      <TestProviders>
        <ExtensionsPreview />
      </TestProviders>
    ));

    const stepper = await screen.findByRole("list", {
      name: "Activation progress",
    });
    expect(within(stepper).getByText("Failed")).toBeVisible();
  });

  it("lists pending requests and approves them, refreshing extensions", async () => {
    extensions = [wasmChannel("pairing")];
    pairing = {
      channel: "whatsapp",
      requests: [
        {
          code: "482913",
          sender_id: "alice",
          created_at: "2026-07-19T00:00:00Z",
        },
      ],
    };

    render(() => (
      <TestProviders>
        <ExtensionsPreview />
      </TestProviders>
    ));

    expect(await screen.findByText("Pending pairing requests")).toBeVisible();
    expect(screen.getByText("482913")).toBeVisible();
    expect(screen.getByText("from alice")).toBeVisible();

    const initialFetchCalls = gateway.requests.filter(
      (request) => request.path === "/api/extensions"
    ).length;
    await userEvent.click(
      screen.getByRole("button", { name: "Approve pairing 482913" })
    );

    await waitFor(() => {
      expect(
        gateway.requests.some(
          (request) =>
            request.method === "POST" &&
            request.path === "/api/pairing/whatsapp/approve" &&
            request.body?.["code"] === "482913"
        )
      ).toBe(true);
    });
    await waitFor(() => {
      expect(
        gateway.requests.filter((request) => request.path === "/api/extensions")
          .length
      ).toBeGreaterThan(initialFetchCalls);
    });
  });

  it("shows the rate-limit error inline when approval is throttled", async () => {
    extensions = [wasmChannel("pairing")];
    pairing = {
      channel: "whatsapp",
      requests: [
        {
          code: "770077",
          sender_id: "mallory",
          created_at: "2026-07-19T00:00:00Z",
        },
      ],
    };
    gateway.fail(
      "POST",
      "/api/pairing/whatsapp/approve",
      429,
      "Too many approvals, retry in 30 seconds"
    );

    render(() => (
      <TestProviders>
        <ExtensionsPreview />
      </TestProviders>
    ));

    await userEvent.click(
      await screen.findByRole("button", { name: "Approve pairing 770077" })
    );

    expect(
      await screen.findByText("Too many approvals, retry in 30 seconds")
    ).toBeVisible();
  });
});
