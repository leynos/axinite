/**
 * @file Memory API wrapper for `/api/memory/*`: workspace tree, file read, search,
 * and write. Used by the `/memory` surface.
 */

import { postJson, requestJson } from "@/lib/api/client";
import type {
  MemoryReadResponse,
  MemorySearchRequest,
  MemorySearchResponse,
  MemoryTreeResponse,
  MemoryWriteRequest,
  MemoryWriteResponse,
} from "@/lib/api/contracts";

/** GET /api/memory/tree: fetch the workspace tree, optionally to `depth`. */
export function fetchMemoryTree(depth?: number): Promise<MemoryTreeResponse> {
  const url = new URL("/api/memory/tree", globalThis.location.origin);
  if (depth !== undefined) {
    url.searchParams.set("depth", String(depth));
  }
  return requestJson<MemoryTreeResponse>(`${url.pathname}${url.search}`);
}

/** GET /api/memory/read: read one workspace file by `path`. */
export function readMemory(path: string): Promise<MemoryReadResponse> {
  const url = new URL("/api/memory/read", globalThis.location.origin);
  url.searchParams.set("path", path);
  return requestJson<MemoryReadResponse>(`${url.pathname}${url.search}`);
}

/** POST /api/memory/search: search workspace memory. */
export function searchMemory(
  request: MemorySearchRequest
): Promise<MemorySearchResponse> {
  return postJson<MemorySearchResponse, MemorySearchRequest>(
    "/api/memory/search",
    request
  );
}

/** POST /api/memory/write: write a workspace memory file. */
export function writeMemory(
  request: MemoryWriteRequest
): Promise<MemoryWriteResponse> {
  return postJson<MemoryWriteResponse, MemoryWriteRequest>(
    "/api/memory/write",
    request
  );
}
