/**
 * @file Session-scoped storage for the gateway bearer token. The token is entered
 * through the auth gate and read by `lib/api/client` for request headers and
 * for the query parameter on SSE URLs.
 */

const STORAGE_KEY = "axinite.gateway-token";

// A denied write must not discard an accepted token. The fallback lasts only
// for this page lifetime; successful storage operations resume session storage.
let fallbackToken: string | null = null;
let usesFallback = false;

function storage(): Storage | undefined {
  try {
    return globalThis.sessionStorage;
  } catch {
    return undefined;
  }
}

/** Return the session token, using page memory when browser storage is denied. */
export function getGatewayToken(): string | null {
  if (usesFallback) return fallbackToken;
  try {
    const currentStorage = storage();
    if (currentStorage) {
      fallbackToken = currentStorage.getItem(STORAGE_KEY);
    }
    return fallbackToken;
  } catch {
    return fallbackToken;
  }
}

/** Store the gateway token for the browser session. */
export function setGatewayToken(token: string): void {
  fallbackToken = token;
  usesFallback = true;
  try {
    const currentStorage = storage();
    if (currentStorage) {
      currentStorage.setItem(STORAGE_KEY, token);
      usesFallback = false;
    }
  } catch {
    // Session storage is optional and can be denied by browser privacy settings.
  }
}

/** Remove the stored gateway token. */
export function clearGatewayToken(): void {
  fallbackToken = null;
  usesFallback = true;
  try {
    const currentStorage = storage();
    if (currentStorage) {
      currentStorage.removeItem(STORAGE_KEY);
      usesFallback = false;
    }
  } catch {
    // Session storage is optional and can be denied by browser privacy settings.
  }
}

/**
 * Append the stored token to `url` as a `token` query parameter.
 *
 * EventSource cannot set request headers, so the gateway accepts the bearer
 * token as a `token` query parameter on its streaming GET endpoints
 * (src/channels/web/auth.rs::allows_query_token_auth).
 */
export function appendTokenToUrl(url: string): string {
  const token = getGatewayToken();
  if (!token) {
    return url;
  }
  const separator = url.includes("?") ? "&" : "?";
  return `${url}${separator}token=${encodeURIComponent(token)}`;
}
