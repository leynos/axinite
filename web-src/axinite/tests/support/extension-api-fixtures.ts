/** @file Stable extension and registry fixtures shared by UI behaviour tests. */

import { GatewayTestServer } from "./gateway-test-server";

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
  tools: ["scrape"],
  version: "0.4.0",
});

/** The registry entry matching {@link firecrawlExtension}. */
export const firecrawlRegistryEntry: Readonly<MockRegistryEntry> =
  Object.freeze({
    description: "Browser automation and retrieval toolkit.",
    display_name: "Firecrawl",
    installed: true,
    kind: "wasm_tool",
    keywords: ["web", "automation"],
    name: "firecrawl",
    version: "0.4.0",
  });

/** Fresh, mutable copies for tests that change their installed state. */
export function cloneFirecrawlExtension(): MockExtension {
  return { ...firecrawlExtension, tools: [...firecrawlExtension.tools] };
}

/** Fresh, mutable registry entry for tests that change installation status. */
export function cloneFirecrawlRegistryEntry(): MockRegistryEntry {
  return {
    ...firecrawlRegistryEntry,
    keywords: [...firecrawlRegistryEntry.keywords],
  };
}

/** Create a gateway with installed and registry state shared across requests. */
export function createFirecrawlGateway(): GatewayTestServer {
  const installedExtensions = [cloneFirecrawlExtension()];
  const registryEntries = [cloneFirecrawlRegistryEntry()];
  const gateway = new GatewayTestServer()
    .route({
      method: "GET",
      path: "/api/extensions",
      handler: async () => Response.json({ extensions: installedExtensions }),
    })
    .respond({
      method: "GET",
      path: "/api/extensions/tools",
      payload: { tools: [] },
    })
    .route({
      method: "GET",
      path: "/api/extensions/registry",
      handler: async () => Response.json({ entries: registryEntries }),
    })
    .route({
      method: "POST",
      path: /^\/api\/extensions\/[^/]+\/remove$/u,
      handler: async (request) => {
        const segments = new URL(request.url).pathname.split("/");
        const name = segments.at(-2) ?? "";
        const remainingExtensions = installedExtensions.filter(
          (extension) => extension.name !== name
        );
        const remainingEntries = registryEntries.map((entry) =>
          entry.name === name ? { ...entry, installed: false } : entry
        );
        installedExtensions.splice(
          0,
          installedExtensions.length,
          ...remainingExtensions
        );
        registryEntries.splice(0, registryEntries.length, ...remainingEntries);
        return Response.json({ message: "Removed", success: true });
      },
    });

  return gateway;
}
