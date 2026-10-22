/**
 * @file Behaviour tests for the extensions route (`ExtensionsPreview`): the
 * installed list, registry, setup and activation flows through controlled
 * responses from an in-process gateway.
 */

import { render, screen, waitFor, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { beforeAll, beforeEach, describe, expect, it } from "vitest";

import { AppProviders } from "@/app/providers";
import { ExtensionsPreview } from "@/components/extensions-preview";
import { DEFAULT_LOCALE } from "@/lib/i18n/supported-locales";
import { createFirecrawlGateway } from "./support/extension-api-fixtures";
import { setupI18nTestHarness } from "./support/i18n-test-runtime";

let gateway: ReturnType<typeof createFirecrawlGateway>;

beforeAll(async () => {
  await setupI18nTestHarness();
});

describe("extensions preview behaviour", () => {
  beforeEach(async () => {
    gateway = createFirecrawlGateway();

    globalThis.localStorage.clear();
    document.documentElement.lang = "";
    document.documentElement.dir = "";
    const runtime = await import("@/lib/i18n/runtime");
    await runtime.default.changeLanguage(DEFAULT_LOCALE);
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
      expect(
        gateway.requests.some(
          (request) =>
            request.method === "POST" &&
            request.path === "/api/extensions/firecrawl/remove"
        )
      ).toBe(true);
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
