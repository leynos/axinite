import { render, screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { axe } from "jest-axe";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { AppProviders } from "@/app/providers";
import { ExtensionsPreview } from "@/components/extensions-preview";
import { DEFAULT_LOCALE } from "@/lib/i18n/supported-locales";
import { setupI18nTestHarness } from "./support/i18n-test-runtime";

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

beforeAll(async () => {
  await setupI18nTestHarness();
});

beforeEach(async () => {
  extensionApiMocks.activateExtension.mockReset();
  extensionApiMocks.fetchExtensionRegistry.mockReset();
  extensionApiMocks.fetchExtensions.mockReset();
  extensionApiMocks.fetchExtensionSetup.mockReset();
  extensionApiMocks.fetchExtensionTools.mockReset();
  extensionApiMocks.installExtension.mockReset();
  extensionApiMocks.removeExtension.mockReset();
  extensionApiMocks.submitExtensionSetup.mockReset();

  window.localStorage.clear();
  document.documentElement.lang = "";
  document.documentElement.dir = "";
  const runtime = await import("@/lib/i18n/runtime");
  await runtime.default.changeLanguage(DEFAULT_LOCALE);

  extensionApiMocks.fetchExtensions.mockResolvedValue({
    extensions: [
      {
        active: true,
        authenticated: true,
        description: "Browser automation and retrieval toolkit.",
        display_name: "Firecrawl",
        has_auth: true,
        kind: "wasm_tool",
        name: "firecrawl",
        needs_setup: false,
        tools: ["scrape"],
        version: "0.4.0",
      },
    ],
  });
  extensionApiMocks.fetchExtensionTools.mockResolvedValue({
    tools: [],
  });
  extensionApiMocks.fetchExtensionRegistry.mockResolvedValue({
    entries: [
      {
        description: "Browser automation and retrieval toolkit.",
        display_name: "Firecrawl",
        installed: true,
        kind: "wasm_tool",
        keywords: ["web", "automation"],
        name: "firecrawl",
        version: "0.4.0",
      },
    ],
  });
  extensionApiMocks.fetchExtensionSetup.mockResolvedValue({
    kind: "wasm_tool",
    name: "firecrawl",
    secrets: [],
  });
  extensionApiMocks.installExtension.mockResolvedValue({
    message: "Installed",
    success: true,
  });
  extensionApiMocks.removeExtension.mockResolvedValue({
    message: "Removed",
    success: true,
  });
  extensionApiMocks.submitExtensionSetup.mockResolvedValue({
    message: "Saved",
    success: true,
  });
});

describe("extensions preview accessibility", () => {
  it("keeps the uninstall confirmation dialog accessible", async () => {
    const user = userEvent.setup();

    render(() => (
      <AppProviders>
        <ExtensionsPreview />
      </AppProviders>
    ));

    await user.click(
      await screen.findByRole("button", { name: "Remove Firecrawl" })
    );

    const dialogResults = await axe(screen.getByRole("alertdialog"), {
      rules: {
        "color-contrast": { enabled: false },
      },
    });

    expect(dialogResults.violations).toHaveLength(0);
  });
});
