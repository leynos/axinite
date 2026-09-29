/**
 * Extensions API wrapper for `/api/extensions/*`: list, tools, registry
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

export function fetchExtensions(): Promise<ExtensionListResponse> {
  return requestJson<ExtensionListResponse>("/api/extensions");
}

export function fetchExtensionTools(): Promise<ToolListResponse> {
  return requestJson<ToolListResponse>("/api/extensions/tools");
}

export function fetchExtensionRegistry(
  query?: string
): Promise<RegistrySearchResponse> {
  const url = new URL("/api/extensions/registry", window.location.origin);
  if (query && query.trim().length > 0) {
    url.searchParams.set("query", query.trim());
  }
  return requestJson<RegistrySearchResponse>(`${url.pathname}${url.search}`);
}

export function installExtension(
  request: InstallExtensionRequest
): Promise<ActionResponse> {
  return postJson<ActionResponse>("/api/extensions/install", request);
}

export function activateExtension(name: string): Promise<ActionResponse> {
  return postJson<ActionResponse>(
    `/api/extensions/${encodeURIComponent(name)}/activate`
  );
}

export function removeExtension(name: string): Promise<ActionResponse> {
  return postJson<ActionResponse>(
    `/api/extensions/${encodeURIComponent(name)}/remove`
  );
}

export function fetchExtensionSetup(
  name: string
): Promise<ExtensionSetupResponse> {
  return requestJson<ExtensionSetupResponse>(
    `/api/extensions/${encodeURIComponent(name)}/setup`
  );
}

export function submitExtensionSetup(
  name: string,
  request: ExtensionSetupRequest
): Promise<ActionResponse> {
  return postJson<ActionResponse>(
    `/api/extensions/${encodeURIComponent(name)}/setup`,
    request
  );
}
