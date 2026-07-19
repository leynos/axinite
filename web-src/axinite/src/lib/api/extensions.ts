import { postJson, requestJson } from "@/lib/api/client";
import type {
  ActionResponse,
  ExtensionListResponse,
  ExtensionSetupRequest,
  ExtensionSetupResponse,
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

export function installExtension(name: string): Promise<ActionResponse> {
  return postJson<ActionResponse>("/api/extensions/install", { name });
}

export function activateExtension(name: string): Promise<ActionResponse> {
  return postJson<ActionResponse>(`/api/extensions/${name}/activate`);
}

export function removeExtension(name: string): Promise<ActionResponse> {
  return postJson<ActionResponse>(`/api/extensions/${name}/remove`);
}

export function fetchExtensionSetup(
  name: string
): Promise<ExtensionSetupResponse> {
  return requestJson<ExtensionSetupResponse>(`/api/extensions/${name}/setup`);
}

export function submitExtensionSetup(
  name: string,
  request: ExtensionSetupRequest
): Promise<ActionResponse> {
  return postJson<ActionResponse>(`/api/extensions/${name}/setup`, request);
}
