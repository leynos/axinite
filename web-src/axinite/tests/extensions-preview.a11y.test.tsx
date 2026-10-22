/**
 * @file Accessibility test (jest-axe) for the extensions route, backed by the
 * in-process gateway. Run by the separate `vitest.a11y.config.ts` project.
 */

import { render, screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { axe } from "jest-axe";
import { beforeAll, beforeEach, describe, expect, it } from "vitest";

import { AppProviders } from "@/app/providers";
import { ExtensionsPreview } from "@/components/extensions-preview";
import { DEFAULT_LOCALE } from "@/lib/i18n/supported-locales";
import { createFirecrawlGateway } from "./support/extension-api-fixtures";
import { setupI18nTestHarness } from "./support/i18n-test-runtime";

beforeAll(async () => {
  await setupI18nTestHarness();
});

beforeEach(async () => {
  createFirecrawlGateway();

  globalThis.localStorage.clear();
  document.documentElement.lang = "";
  document.documentElement.dir = "";
  const runtime = await import("@/lib/i18n/runtime");
  await runtime.default.changeLanguage(DEFAULT_LOCALE);
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
