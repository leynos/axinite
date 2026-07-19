import { render, screen, waitFor, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { AppProviders } from "@/app/providers";
import { ExtensionsPreview } from "@/components/extensions-preview";
import { DEFAULT_LOCALE } from "@/lib/i18n/supported-locales";
import { setupI18nTestHarness } from "./support/i18n-test-runtime";

type MockExtension = {
  active: boolean;
  authenticated: boolean;
  description: string;
  display_name: string;
  has_auth: boolean;
  kind: string;
  name: string;
  needs_setup: boolean;
  tools: string[];
  version: string;
};

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

describe("extensions preview behaviour", () => {
  let installedExtensions: MockExtension[];
  let registryEntries: Array<{
    description: string;
    display_name: string;
    installed: boolean;
    kind: string;
    keywords: string[];
    name: string;
    version: string;
  }>;

  beforeEach(async () => {
    installedExtensions = [
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
    ];
    registryEntries = [
      {
        description: "Browser automation and retrieval toolkit.",
        display_name: "Firecrawl",
        installed: true,
        kind: "wasm_tool",
        keywords: ["web", "automation"],
        name: "firecrawl",
        version: "0.4.0",
      },
    ];

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

    extensionApiMocks.fetchExtensions.mockImplementation(async () => ({
      extensions: installedExtensions,
    }));
    extensionApiMocks.fetchExtensionTools.mockResolvedValue({
      tools: [],
    });
    extensionApiMocks.fetchExtensionRegistry.mockImplementation(async () => ({
      entries: registryEntries,
    }));
    extensionApiMocks.fetchExtensionSetup.mockResolvedValue({
      kind: "wasm_tool",
      name: "firecrawl",
      secrets: [],
    });
    extensionApiMocks.installExtension.mockResolvedValue({
      message: "Installed",
      success: true,
    });
    extensionApiMocks.submitExtensionSetup.mockResolvedValue({
      message: "Saved",
      success: true,
    });
    extensionApiMocks.removeExtension.mockImplementation(
      async (name: string) => {
        installedExtensions = installedExtensions.filter(
          (extension) => extension.name !== name
        );
        registryEntries = registryEntries.map((entry) =>
          entry.name === name ? { ...entry, installed: false } : entry
        );

        return {
          message: "Removed",
          success: true,
        };
      }
    );
  });

  it("confirms removal and makes built-in wasm extensions installable again", async () => {
    const user = userEvent.setup();

    render(() => (
      <AppProviders>
        <ExtensionsPreview />
      </AppProviders>
    ));

    expect(
      await screen.findByRole("table", { name: "Available WASM extensions" })
    ).toBeVisible();
    expect(screen.getByRole("columnheader", { name: "Name" })).toBeVisible();

    await screen.findByRole("button", { name: "Remove Firecrawl" });
    const firecrawlRow = screen.getByRole("row", {
      name: /Firecrawl Browser automation and retrieval toolkit\. Installed/,
    });
    expect(
      within(firecrawlRow).getByRole("button", { name: "Installed" })
    ).toBeDisabled();

    await user.click(screen.getByRole("button", { name: "Remove Firecrawl" }));

    const dialog = await screen.findByRole("alertdialog", {
      name: "Remove Firecrawl?",
    });
    expect(
      within(dialog).getByText(
        "Built-in WASM extensions become available to install again after removal."
      )
    ).toBeVisible();

    await user.click(
      within(dialog).getByRole("button", { name: "Remove extension" })
    );

    await waitFor(() => {
      expect(extensionApiMocks.removeExtension).toHaveBeenCalledWith(
        "firecrawl"
      );
    });
    await waitFor(() => {
      expect(
        screen.queryByRole("button", { name: "Remove Firecrawl" })
      ).toBeNull();
    });
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Install" })).toBeEnabled();
    });
  });
});
