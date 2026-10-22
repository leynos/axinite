/**
 * @file Extensions API wrapper for `/api/extensions/*`: list, tools, registry
 * search, install, activate, remove, and secret setup. Used by the
 * `/extensions` surface.
 */

import { postJson, requestJson } from "@/lib/api/client";
import type {
  ActionResponse,
  ExtensionListResponse,
  ExtensionSetupRequest,
  ExtensionSetupResponse,
  InstallExtensionRequest,
  RegistrySearchResponse,
  ToolListResponse,
} from "@/lib/api/contracts";

/** GET /api/extensions: list installed extensions. */
export function fetchExtensions(): Promise<ExtensionListResponse> {
  return requestJson<ExtensionListResponse>("/api/extensions");
}

/** GET /api/extensions/tools: list tools provided by extensions. */
export function fetchExtensionTools(): Promise<ToolListResponse> {
  return requestJson<ToolListResponse>("/api/extensions/tools");
}

/** GET /api/extensions/registry: search the registry (`query` optional). */
export function fetchExtensionRegistry(
  query?: string
): Promise<RegistrySearchResponse> {
  const url = new URL("/api/extensions/registry", globalThis.location.origin);
  if (query && query.trim().length > 0) {
    url.searchParams.set("query", query.trim());
  }
  return requestJson<RegistrySearchResponse>(`${url.pathname}${url.search}`);
}

/** POST /api/extensions/install: install an extension. */
export function installExtension(
  request: InstallExtensionRequest
): Promise<ActionResponse> {
  return postJson<ActionResponse, InstallExtensionRequest>(
    "/api/extensions/install",
    request
  );
}

/** POST /api/extensions/{name}/activate: activate an installed extension. */
export function activateExtension(name: string): Promise<ActionResponse> {
  return postJson<ActionResponse>(
    `/api/extensions/${encodeURIComponent(name)}/activate`
  );
}

/** POST /api/extensions/{name}/remove: remove an extension. */
export function removeExtension(name: string): Promise<ActionResponse> {
  return postJson<ActionResponse>(
    `/api/extensions/${encodeURIComponent(name)}/remove`
  );
}

/** GET /api/extensions/{name}/setup: fetch an extension's setup fields. */
export function fetchExtensionSetup(
  name: string
): Promise<ExtensionSetupResponse> {
  return requestJson<ExtensionSetupResponse>(
    `/api/extensions/${encodeURIComponent(name)}/setup`
  );
}

/** POST /api/extensions/{name}/setup: submit an extension's setup values. */
export function submitExtensionSetup(
  name: string,
  request: ExtensionSetupRequest
): Promise<ActionResponse> {
  return postJson<ActionResponse, ExtensionSetupRequest>(
    `/api/extensions/${encodeURIComponent(name)}/setup`,
    request
  );
}
