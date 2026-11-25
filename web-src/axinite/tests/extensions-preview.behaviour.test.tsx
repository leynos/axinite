/**
 * @file Behaviour tests for the extensions route (`ExtensionsPreview`): the
 * installed list, registry, setup and activation flows through controlled
 * responses from an in-process gateway.
 */

import { render, screen, waitFor, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { beforeAll, beforeEach, describe, expect, it } from "vitest";

import { ExtensionsPreview } from "@/components/extensions-preview";
import { DEFAULT_LOCALE } from "@/lib/i18n/supported-locales";
import {
  createFirecrawlGateway,
  createSetupGateway,
} from "./support/extension-api-fixtures";
import { setupI18nTestHarness } from "./support/i18n-test-runtime";
import { TestProviders } from "./support/test-providers";

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
      <TestProviders>
        <ExtensionsPreview />
      </TestProviders>
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

  it("renders endpoint controls and submits replacements and clears", async () => {
    const user = userEvent.setup();
    const setup = createSetupGateway();
    gateway = setup.gateway;

    render(() => (
      <TestProviders>
        <ExtensionsPreview />
      </TestProviders>
    ));

    await user.click(await screen.findByRole("button", { name: "Configure" }));

    const calendar = await screen.findByLabelText("Calendar endpoint URL");
    expect(calendar).toHaveAttribute("type", "url");
    expect(calendar).toHaveAttribute(
      "placeholder",
      "(stored — enter a new URL to replace)"
    );
    expect(calendar).toHaveValue("");

    const contacts = screen.getByLabelText(/Contacts endpoint URL/u);
    expect(contacts).toHaveAttribute("placeholder", "Contacts endpoint URL");

    await user.type(calendar, "https://mail.example.com/caldav");
    await user.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(setup.submissions).toHaveLength(1);
    });
    expect(setup.submissions[0]?.endpoints?.set).toEqual({
      calendar: "https://mail.example.com/caldav",
    });
    expect(setup.submissions[0]?.endpoints?.clear ?? []).toEqual([]);

    const clear = screen.getByRole("checkbox", { name: "Clear stored value" });
    await user.click(clear);
    await user.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(setup.submissions).toHaveLength(2);
    });
    expect(setup.submissions[1]?.endpoints?.clear).toEqual(["calendar"]);
    expect(setup.submissions[1]?.endpoints?.set ?? {}).toEqual({});
  });
});
