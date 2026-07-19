export const DEPLOY_BASE_PATH = "/";

export function normalizeBasePath(rawBase: string | undefined): string {
  const candidate = rawBase && rawBase.length > 0 ? rawBase : "/";
  const withLeading = candidate.startsWith("/") ? candidate : `/${candidate}`;
  return withLeading.endsWith("/") ? withLeading : `${withLeading}/`;
}

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
