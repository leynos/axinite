import { postJson, requestJson } from "@/lib/api/client";
import type {
  MemoryReadResponse,
  MemorySearchRequest,
  MemorySearchResponse,
  MemoryTreeResponse,
  MemoryWriteRequest,
  MemoryWriteResponse,
} from "@/lib/api/contracts";

export function fetchMemoryTree(depth?: number): Promise<MemoryTreeResponse> {
  const url = new URL("/api/memory/tree", window.location.origin);
  if (typeof depth === "number") {
    url.searchParams.set("depth", String(depth));
  }
  return requestJson<MemoryTreeResponse>(`${url.pathname}${url.search}`);
}

export function readMemory(path: string): Promise<MemoryReadResponse> {
  const url = new URL("/api/memory/read", window.location.origin);
  url.searchParams.set("path", path);
  return requestJson<MemoryReadResponse>(`${url.pathname}${url.search}`);
}

export function searchMemory(
  request: MemorySearchRequest
): Promise<MemorySearchResponse> {
  return postJson<MemorySearchResponse>("/api/memory/search", request);
}

export function writeMemory(
  request: MemoryWriteRequest
): Promise<MemoryWriteResponse> {
  return postJson<MemoryWriteResponse>("/api/memory/write", request);
}
