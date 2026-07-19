import { existsSync, statSync } from "node:fs";
import path from "node:path";

import { isStreamingApiPath } from "./streaming-routes";

const previewPort = Number(process.env.PREVIEW_PORT ?? "2020");
const apiPort = Number(process.env.MOCK_API_PORT ?? "8787");
const distDir = path.join(process.cwd(), "dist");

function fileExists(filePath: string): boolean {
  if (!existsSync(filePath)) {
    return false;
  }

  return statSync(filePath).isFile();
}

function resolveStaticPath(pathname: string): string | null {
  const cleanPath = pathname === "/" ? "/index.html" : pathname;
  const candidates = [
    path.join(distDir, cleanPath.replace(/^\/+/, "")),
    path.join(distDir, cleanPath.replace(/^\/+/, ""), "index.html"),
  ];

  if (!path.extname(cleanPath)) {
    candidates.push(
      path.join(distDir, `${cleanPath.replace(/^\/+/, "")}.html`),
      path.join(distDir, cleanPath.replace(/^\/+/, ""), "index.html")
    );
  }

  for (const candidate of candidates) {
    if (fileExists(candidate)) {
      return candidate;
    }
  }

  return null;
}

async function proxyRequest(request: Request): Promise<Response> {
  const url = new URL(request.url);
  const upstream = new URL(url.pathname + url.search, `http://127.0.0.1:${apiPort}`);
  const response = await fetch(upstream, {
    method: request.method,
    headers: request.headers,
    body:
      request.method === "GET" || request.method === "HEAD"
        ? undefined
        : request.body,
  });

  return new Response(response.body, {
    headers: response.headers,
    status: response.status,
    statusText: response.statusText,
  });
}

function serveStatic(pathname: string): Response {
  const resolvedPath = resolveStaticPath(pathname);
  if (!resolvedPath) {
    return new Response("Not found", { status: 404 });
  }
  return new Response(Bun.file(resolvedPath));
}

const server = Bun.serve({
  port: previewPort,
  async fetch(request, server) {
    const url = new URL(request.url);
    if (isStreamingApiPath(url.pathname)) {
      server.timeout(request, 0);
    }
    if (url.pathname.startsWith("/api/")) {
      return proxyRequest(request);
    }

    return serveStatic(url.pathname);
  },
});

console.log(
  `[preview] serving dist on http://localhost:${server.port ?? previewPort} with API proxy http://localhost:${apiPort}`
);
