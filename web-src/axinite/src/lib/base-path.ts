export const GITHUB_PAGES_BASE_PATH = "/axinite-mockup/";

export function normaliseBasePath(rawBase: string | undefined): string {
  const candidate = rawBase && rawBase.length > 0 ? rawBase : "/";
  const withLeading = candidate.startsWith("/") ? candidate : `/${candidate}`;
  return withLeading.endsWith("/") ? withLeading : `${withLeading}/`;
}

export function buildAppPath(
  rawBase: string | undefined,
  path: string
): string {
  const basePath = normaliseBasePath(rawBase);
  const trimmedPath = path.replace(/^\/+/, "");

  if (trimmedPath.length === 0) {
    return basePath;
  }

  return `${basePath}${trimmedPath}`;
}
