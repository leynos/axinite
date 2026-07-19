import { render, screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { axe } from "jest-axe";
import { beforeAll, describe, expect, it } from "vitest";

import { AppProviders } from "@/app/providers";
import { ShellChrome } from "@/components/app-shell";
import { setupI18nTestHarness } from "./support/i18n-test-runtime";

beforeAll(async () => {
  await setupI18nTestHarness();
});

describe("app shell accessibility", () => {
  it("keeps the shell and logs dialog accessible", async () => {
    const user = userEvent.setup();
    const { container } = render(() => (
      <AppProviders>
        <ShellChrome activePath="/chat" usePlainLinks>
          <div>Child</div>
        </ShellChrome>
      </AppProviders>
    ));

    const shellResults = await axe(container, {
      rules: {
        "color-contrast": { enabled: false },
      },
    });
    expect(shellResults.violations).toHaveLength(0);

    await user.click(screen.getByRole("button", { name: "Logs" }));

    const dialogResults = await axe(screen.getByRole("dialog"), {
      rules: {
        "color-contrast": { enabled: false },
      },
    });
    expect(dialogResults.violations).toHaveLength(0);
  });
});
