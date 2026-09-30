import { render, screen, waitFor } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { AppProviders } from "@/app/providers";
import { MemoryPreview } from "@/components/memory-preview";
import { DEFAULT_LOCALE } from "@/lib/i18n/supported-locales";
import { setupI18nTestHarness } from "./support/i18n-test-runtime";

const memoryApiMocks = vi.hoisted(() => ({
  fetchMemoryTree: vi.fn(),
  readMemory: vi.fn(),
  searchMemory: vi.fn(),
  writeMemory: vi.fn(),
}));

vi.mock("@/lib/api/memory", () => memoryApiMocks);

beforeAll(async () => {
  await setupI18nTestHarness();
});

beforeEach(async () => {
  for (const mock of Object.values(memoryApiMocks)) {
    mock.mockReset();
  }
  window.localStorage.clear();
  const runtime = await import("@/lib/i18n/runtime");
  await runtime.default.changeLanguage(DEFAULT_LOCALE);

  memoryApiMocks.fetchMemoryTree.mockResolvedValue({
    entries: [
      { path: "notes/alpha.md", is_dir: false },
      { path: "notes/beta.md", is_dir: false },
    ],
  });
  memoryApiMocks.readMemory.mockImplementation(async (path: string) => ({
    path,
    content: `Body of ${path}`,
    updated_at: null,
  }));
  memoryApiMocks.writeMemory.mockResolvedValue({ path: "notes/alpha.md" });
});

describe("memory preview editing", () => {
  it("locks document selection while a draft is open", async () => {
    render(() => (
      <AppProviders>
        <MemoryPreview />
      </AppProviders>
    ));

    const beta = await screen.findByRole("button", { name: "beta.md" });
    await screen.findByText("Body of notes/alpha.md");
    expect(beta).toBeEnabled();

    await userEvent.click(screen.getByRole("button", { name: "Edit" }));
    expect(beta).toBeDisabled();

    // Switching is impossible, so Save writes the draft to the open document.
    await userEvent.click(beta);
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(memoryApiMocks.writeMemory).toHaveBeenCalledWith(
        expect.objectContaining({ path: "notes/alpha.md" })
      );
    });

    await waitFor(() => {
      expect(beta).toBeEnabled();
    });
  });
});
