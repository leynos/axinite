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

/** Setup submission captured by {@link createSetupGateway}. */
export type CapturedSetupSubmission = {
  secrets: Record<string, string>;
  endpoints?: {
    set?: Record<string, string>;
    clear?: string[];
  };
};

/** The synthetic tool this setup fixture installs: one secret, two endpoints. */
export const jmapExtension: Readonly<MockExtension> = Object.freeze({
  active: false,
  authenticated: false,
  description: "Mailbox reads and message triage.",
  display_name: "JMAP Mail",
  has_auth: false,
  kind: "wasm_tool",
  name: "jmap",
  needs_setup: true,
  tools: [],
  version: "0.1.0",
});

/** The gateway and captured submissions from {@link createSetupGateway}. */
export type SetupGatewayFixture = {
  gateway: GatewayTestServer;
  submissions: CapturedSetupSubmission[];
};

/**
 * Create a gateway whose setup route serves one credential and two endpoints,
 * and captures the submission bodies it receives.
 */
export function createSetupGateway(): SetupGatewayFixture {
  const submissions: CapturedSetupSubmission[] = [];
  const setupResponse = {
    name: "jmap",
    kind: "wasm_tool",
    secrets: [
      {
        name: "token",
        prompt: "API token",
        optional: false,
        provided: true,
        auto_generate: false,
      },
    ],
    endpoints: [
      {
        name: "calendar",
        kind: "caldav",
        prompt: "Calendar endpoint URL",
        optional: false,
        state: "configured",
      },
      {
        name: "contacts",
        kind: "carddav",
        prompt: "Contacts endpoint URL",
        optional: true,
        state: "missing",
      },
    ],
    ready: true,
    blockers: [],
  };
  const gateway = new GatewayTestServer()
    .route({
      method: "GET",
      path: "/api/extensions",
      handler: async () =>
        Response.json({ extensions: [{ ...jmapExtension }] }),
    })
    .respond({
      method: "GET",
      path: "/api/extensions/tools",
      payload: { tools: [] },
    })
    .respond({
      method: "GET",
      path: "/api/extensions/registry",
      payload: { entries: [] },
    })
    .route({
      method: "GET",
      path: "/api/extensions/jmap/setup",
      handler: async () => Response.json(setupResponse),
    })
    .route({
      method: "POST",
      path: "/api/extensions/jmap/setup",
      handler: async (request) => {
        // SAFETY: this test fixture owns both ends of the request, and the
        // component under test builds the body from the typed contract.
        const body = (await request.json()) as CapturedSetupSubmission;
        submissions.push(body);
        return Response.json({
          success: true,
          message: "Extension setup saved.",
          activated: true,
          ready: true,
          outcomes: [],
          endpoints: setupResponse.endpoints,
          blockers: [],
        });
      },
    });
  return { gateway, submissions };
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
