/**
 * Shared stand-ins for `@/lib/api/extensions` used by the extensions specs.
 *
 * Each spec registers the mocks with
 * `vi.mock("@/lib/api/extensions", async () => (await import(...)).extensionApiMocks)`
 * so the factory and the spec share one module instance (Vitest isolates
 * modules per test file), then calls `resetExtensionApiMocks()` and
 * `applyDefaultExtensionMocks()` in `beforeEach` before any spec-specific
 * overrides.
 */
import { vi } from "vitest";

export type MockExtension = {
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

export type MockRegistryEntry = {
  description: string;
  display_name: string;
  installed: boolean;
  kind: string;
  keywords: string[];
  name: string;
  version: string;
};

export const extensionApiMocks = {
  activateExtension: vi.fn(),
  fetchExtensionRegistry: vi.fn(),
  fetchExtensions: vi.fn(),
  fetchExtensionSetup: vi.fn(),
  fetchExtensionTools: vi.fn(),
  installExtension: vi.fn(),
  removeExtension: vi.fn(),
  submitExtensionSetup: vi.fn(),
};

/** The installed Firecrawl WASM tool both specs render. */
export const firecrawlExtension: Readonly<MockExtension> = Object.freeze({
  active: true,
  authenticated: true,
  description: "Browser automation and retrieval toolkit.",
  display_name: "Firecrawl",
  has_auth: true,
  kind: "wasm_tool",
  name: "firecrawl",
  needs_setup: false,
  tools: Object.freeze(["scrape"]) as unknown as string[],
  version: "0.4.0",
});

/** The registry entry matching {@link firecrawlExtension}. */
export const firecrawlRegistryEntry: Readonly<MockRegistryEntry> =
  Object.freeze({
    description: "Browser automation and retrieval toolkit.",
    display_name: "Firecrawl",
    installed: true,
    kind: "wasm_tool",
    keywords: Object.freeze(["web", "automation"]) as unknown as string[],
    name: "firecrawl",
    version: "0.4.0",
  });

/** Fresh, mutable copies of the fixtures for specs that change them. */
export function cloneFirecrawlExtension(): MockExtension {
  return { ...firecrawlExtension, tools: [...firecrawlExtension.tools] };
}

export function cloneFirecrawlRegistryEntry(): MockRegistryEntry {
  return {
    ...firecrawlRegistryEntry,
    keywords: [...firecrawlRegistryEntry.keywords],
  };
}

export function resetExtensionApiMocks(): void {
  for (const mock of Object.values(extensionApiMocks)) {
    mock.mockReset();
  }
}

/** Resolve every API call with the Firecrawl fixtures and success bodies. */
export function applyDefaultExtensionMocks(): void {
  extensionApiMocks.fetchExtensions.mockResolvedValue({
    extensions: [cloneFirecrawlExtension()],
  });
  extensionApiMocks.fetchExtensionTools.mockResolvedValue({ tools: [] });
  extensionApiMocks.fetchExtensionRegistry.mockResolvedValue({
    entries: [cloneFirecrawlRegistryEntry()],
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
}
