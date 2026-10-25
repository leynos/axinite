/**
 * @file Session-scoped storage for the gateway bearer token. The token is entered
 * through the auth gate and read by `lib/api/client` for request headers and
 * for the query parameter on SSE URLs.
 */

const STORAGE_KEY = "axinite.gateway-token";

function storage(): Storage | undefined {
  try {
    return globalThis.sessionStorage;
  } catch {
    return undefined;
  }
}

/** Return the stored gateway token, or `null` when none is stored. */
export function getGatewayToken(): string | null {
  try {
    return storage()?.getItem(STORAGE_KEY) ?? null;
  } catch {
    return null;
  }
}

/** Store the gateway token for the browser session. */
export function setGatewayToken(token: string): void {
  try {
    storage()?.setItem(STORAGE_KEY, token);
  } catch {
    // Session storage is optional and can be denied by browser privacy settings.
  }
}

/** Remove the stored gateway token. */
export function clearGatewayToken(): void {
  try {
    storage()?.removeItem(STORAGE_KEY);
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
