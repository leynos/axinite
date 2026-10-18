/**
 * @file Helpers for the deployment base path (Vite `BASE_URL`). They normalize it
 * to a leading and trailing slash so the router, locale loader, and shell
 * links resolve correctly when the SPA is served under a prefix.
 */

export const DEPLOY_BASE_PATH = "/";

/**
 * Adds leading and trailing slashes to a base path, defaulting empty input to
 * `/`. Other path segments are left unchanged.
 */
export function normalizeBasePath(rawBase: string | undefined): string {
  const candidate = rawBase && rawBase.length > 0 ? rawBase : "/";
  const withLeading = candidate.startsWith("/") ? candidate : `/${candidate}`;
  return withLeading.endsWith("/") ? withLeading : `${withLeading}/`;
}

/** Joins a normalized base path to a path with any leading slashes removed. */
export function buildAppPath(
  rawBase: string | undefined,
  path: string
): string {
  const basePath = normalizeBasePath(rawBase);
  const trimmedPath = path.replace(/^\/+/, "");

  if (trimmedPath.length === 0) {
    return basePath;
  }

  return `${basePath}${trimmedPath}`;
}
