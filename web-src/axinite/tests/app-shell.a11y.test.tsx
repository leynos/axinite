/**
 * @file Accessibility test (jest-axe) for the app shell chrome and its
 * navigation. Run by the separate `vitest.a11y.config.ts` project, not the
 * default unit and behaviour suite.
 */

import { render, screen } from "@solidjs/testing-library";
import { axe } from "jest-axe";
import { beforeAll, describe, expect, it } from "vitest";

import { AppProviders } from "@/app/providers";
import { ShellChrome } from "@/components/app-shell";
import { setupI18nTestHarness } from "./support/i18n-test-runtime";

beforeAll(async () => {
  await setupI18nTestHarness();
});

describe("app shell accessibility", () => {
  it("keeps the shell accessible, including the logs nav entry", async () => {
    const { container } = render(() => (
      <AppProviders>
        <ShellChrome activePath="/chat" usePlainLinks>
          <div>Child</div>
        </ShellChrome>
      </AppProviders>
    ));

    expect(screen.getByRole("link", { name: "Logs" })).toBeVisible();

    const shellResults = await axe(container, {
      rules: {
        "color-contrast": { enabled: false },
      },
    });
    expect(shellResults.violations).toHaveLength(0);
  });
});
