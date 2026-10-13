/**
 * @file Static preview transport for the built SPA. Handler dependencies are
 * injectable; only direct execution starts Bun and reads runtime configuration.
 */
import { existsSync, statSync } from "node:fs";
import path from "node:path";
import { parsePort } from "./ports";
import { isStreamingApiPath } from "./streaming-routes";

/** Filesystem and upstream HTTP operations needed by the preview transport. */
export type PreviewDependencies = {
  isFile: (filePath: string) => boolean;
  readFile: (filePath: string) => BodyInit;
  fetchUpstream: (url: URL, init: RequestInit) => Promise<Response>;
};

/** Request deadline capability supplied by Bun, or a test adapter. */
export type PreviewDeadline = {
  timeout: (request: Request, seconds: number) => void;
};

/** Reports missing paths as false and permits only regular files. */
function fileExists(filePath: string): boolean {
  return existsSync(filePath) && statSync(filePath).isFile();
}

// Preserve gateway-style precedence: exact file, directory index, optional
// extension-less sibling HTML, then SPA shell. Assets with extensions never
// fall back to HTML, so a missing script fails visibly instead of parsing HTML.
function candidatePaths(distDir: string, pathname: string): string[] {
  const cleanPath = pathname === "/" ? "/index.html" : pathname;
  const relative = cleanPath.replace(/^\/+/, "");
  return [
    path.join(distDir, relative),
    path.join(distDir, relative, "index.html"),
    ...(path.extname(cleanPath)
      ? []
      : [
          path.join(distDir, `${relative}.html`),
          path.join(distDir, "index.html"),
        ]),
  ];
}

/**
 * Forward an API request to the mock server on loopback, preserving its path,
 * query, method, and headers. GET and HEAD carry no body; other methods pass
 * through the incoming body. The response body, headers, status, and status
 * text are returned unchanged, while upstream fetch failures propagate.
 */
async function proxyRequest(
  request: Request,
  apiPort: number,
  deps: PreviewDependencies
): Promise<Response> {
  const url = new URL(request.url);
  const upstream = new URL(
    url.pathname + url.search,
    `http://127.0.0.1:${apiPort}`
  );
  const response = await deps.fetchUpstream(upstream, {
    method: request.method,
    headers: request.headers,
    body:
      request.method === "GET" || request.method === "HEAD"
        ? null
        : request.body,
  });
  return new Response(response.body, {
    headers: response.headers,
    status: response.status,
    statusText: response.statusText,
  });
}

/**
 * Creates an import-safe preview handler. Static routes use ordered SPA
 * fallbacks; API routes preserve query, method, headers, body and upstream
 * response metadata. Only SSE routes disable the supplied request deadline.
 */
export function createPreviewHandler(
  options: { distDir: string; apiPort: number },
  deps: PreviewDependencies
): (request: Request, server: PreviewDeadline) => Promise<Response> {
  return async (request, server) => {
    const pathname = new URL(request.url).pathname;
    if (isStreamingApiPath(pathname)) server.timeout(request, 0);
    if (pathname.startsWith("/api/"))
      return proxyRequest(request, options.apiPort, deps);
    const resolved = candidatePaths(options.distDir, pathname).find(
      deps.isFile
    );
    return resolved
      ? new Response(deps.readFile(resolved))
      : new Response("Not found", { status: 404 });
  };
}

if (import.meta.main) {
  const previewPort = parsePort(
    "PREVIEW_PORT",
    process.env["PREVIEW_PORT"],
    2020
  );
  const apiPort = parsePort(
    "MOCK_API_PORT",
    process.env["MOCK_API_PORT"],
    8787
  );
  const handler = createPreviewHandler(
    { distDir: path.join(process.cwd(), "dist"), apiPort },
    {
      isFile: fileExists,
      readFile: (filePath) => Bun.file(filePath),
      fetchUpstream: (url, init) => fetch(url, init),
    }
  );
  const server = Bun.serve({ port: previewPort, fetch: handler });
  console.log(
    `[preview] serving dist on http://localhost:${server.port ?? previewPort} with API proxy http://localhost:${apiPort}`
  );
}
