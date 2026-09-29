import { render, screen, waitFor, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { AppProviders } from "@/app/providers";
import { ExtensionsPreview } from "@/components/extensions-preview";
import { DEFAULT_LOCALE } from "@/lib/i18n/supported-locales";
import {
  applyDefaultExtensionMocks,
  cloneFirecrawlExtension,
  cloneFirecrawlRegistryEntry,
  extensionApiMocks,
  type MockExtension,
  type MockRegistryEntry,
  resetExtensionApiMocks,
} from "./support/extension-api-mocks";
import { setupI18nTestHarness } from "./support/i18n-test-runtime";

vi.mock(
  "@/lib/api/extensions",
  async () => (await import("./support/extension-api-mocks")).extensionApiMocks
);

beforeAll(async () => {
  await setupI18nTestHarness();
});

describe("extensions preview behaviour", () => {
  let installedExtensions: MockExtension[];
  let registryEntries: MockRegistryEntry[];

  beforeEach(async () => {
    installedExtensions = [cloneFirecrawlExtension()];
    registryEntries = [cloneFirecrawlRegistryEntry()];

    resetExtensionApiMocks();

    window.localStorage.clear();
    document.documentElement.lang = "";
    document.documentElement.dir = "";
    const runtime = await import("@/lib/i18n/runtime");
    await runtime.default.changeLanguage(DEFAULT_LOCALE);

    applyDefaultExtensionMocks();
    extensionApiMocks.fetchExtensions.mockImplementation(async () => ({
      extensions: installedExtensions,
    }));
    extensionApiMocks.fetchExtensionRegistry.mockImplementation(async () => ({
      entries: registryEntries,
    }));
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
