import { render, screen, waitFor, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { ExtensionsPreview } from "@/components/extensions-preview";
import { DEFAULT_LOCALE } from "@/lib/i18n/supported-locales";
import { setupI18nTestHarness } from "./support/i18n-test-runtime";
import { TestProviders } from "./support/test-providers";

const extensionApiMocks = vi.hoisted(() => ({
  activateExtension: vi.fn(),
  fetchExtensionRegistry: vi.fn(),
  fetchExtensions: vi.fn(),
  fetchExtensionSetup: vi.fn(),
  fetchExtensionTools: vi.fn(),
  installExtension: vi.fn(),
  removeExtension: vi.fn(),
  submitExtensionSetup: vi.fn(),
}));

const pairingApiMocks = vi.hoisted(() => ({
  approvePairing: vi.fn(),
  fetchPairingRequests: vi.fn(),
}));

vi.mock("@/lib/api/extensions", () => ({
  activateExtension: extensionApiMocks.activateExtension,
  fetchExtensionRegistry: extensionApiMocks.fetchExtensionRegistry,
  fetchExtensions: extensionApiMocks.fetchExtensions,
  fetchExtensionSetup: extensionApiMocks.fetchExtensionSetup,
  fetchExtensionTools: extensionApiMocks.fetchExtensionTools,
  installExtension: extensionApiMocks.installExtension,
  removeExtension: extensionApiMocks.removeExtension,
  submitExtensionSetup: extensionApiMocks.submitExtensionSetup,
}));

vi.mock("@/lib/api/pairing", () => ({
  approvePairing: pairingApiMocks.approvePairing,
  fetchPairingRequests: pairingApiMocks.fetchPairingRequests,
}));

function wasmChannel(activationStatus: string) {
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
  for (const mock of Object.values(extensionApiMocks)) {
    mock.mockReset();
  }
  for (const mock of Object.values(pairingApiMocks)) {
    mock.mockReset();
  }

  window.localStorage.clear();
  document.documentElement.lang = "";
  document.documentElement.dir = "";
  const runtime = await import("@/lib/i18n/runtime");
  await runtime.default.changeLanguage(DEFAULT_LOCALE);

  extensionApiMocks.fetchExtensionTools.mockResolvedValue({ tools: [] });
  extensionApiMocks.fetchExtensionRegistry.mockResolvedValue({ entries: [] });
  extensionApiMocks.fetchExtensionSetup.mockResolvedValue({
    kind: "wasm_channel",
    name: "whatsapp",
    secrets: [],
  });
  pairingApiMocks.fetchPairingRequests.mockResolvedValue({
    channel: "whatsapp",
    requests: [],
  });
});

describe("extension pairing and activation stepper", () => {
  it("shows the awaiting-pairing stepper state for a pairing channel", async () => {
    extensionApiMocks.fetchExtensions.mockResolvedValue({
      extensions: [wasmChannel("pairing")],
    });

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
    extensionApiMocks.fetchExtensions.mockResolvedValue({
      extensions: [wasmChannel("failed")],
    });

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
    extensionApiMocks.fetchExtensions.mockResolvedValue({
      extensions: [wasmChannel("pairing")],
    });
    pairingApiMocks.fetchPairingRequests.mockResolvedValue({
      channel: "whatsapp",
      requests: [
        {
          code: "482913",
          sender_id: "alice",
          created_at: "2026-07-19T00:00:00Z",
        },
      ],
    });
    pairingApiMocks.approvePairing.mockResolvedValue({
      success: true,
      message: "Pairing approved",
    });

    render(() => (
      <TestProviders>
        <ExtensionsPreview />
      </TestProviders>
    ));

    expect(await screen.findByText("Pending pairing requests")).toBeVisible();
    expect(screen.getByText("482913")).toBeVisible();
    expect(screen.getByText("from alice")).toBeVisible();

    const initialFetchCalls =
      extensionApiMocks.fetchExtensions.mock.calls.length;
    await userEvent.click(
      screen.getByRole("button", { name: "Approve pairing 482913" })
    );

    await waitFor(() => {
      expect(pairingApiMocks.approvePairing).toHaveBeenCalledWith(
        "whatsapp",
        "482913"
      );
    });
    await waitFor(() => {
      expect(
        extensionApiMocks.fetchExtensions.mock.calls.length
      ).toBeGreaterThan(initialFetchCalls);
    });
  });

  it("shows the rate-limit error inline when approval is throttled", async () => {
    extensionApiMocks.fetchExtensions.mockResolvedValue({
      extensions: [wasmChannel("pairing")],
    });
    pairingApiMocks.fetchPairingRequests.mockResolvedValue({
      channel: "whatsapp",
      requests: [
        {
          code: "770077",
          sender_id: "mallory",
          created_at: "2026-07-19T00:00:00Z",
        },
      ],
    });
    pairingApiMocks.approvePairing.mockRejectedValue(
      new Error("Too many approvals, retry in 30 seconds")
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
