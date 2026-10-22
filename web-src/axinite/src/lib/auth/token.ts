/**
 * @file Session-scoped storage for the gateway bearer token. The token is entered
 * through the auth gate and read by `lib/api/client` for request headers and
 * for the query parameter on SSE URLs.
 */

const STORAGE_KEY = "axinite.gateway-token";

function storage(): Storage {
  return globalThis.sessionStorage;
}

/** Return the stored gateway token, or `null` when none is stored. */
export function getGatewayToken(): string | null {
  return storage().getItem(STORAGE_KEY);
}

/** Store the gateway token for the browser session. */
export function setGatewayToken(token: string): void {
  storage().setItem(STORAGE_KEY, token);
}

/** Remove the stored gateway token. */
export function clearGatewayToken(): void {
  storage().removeItem(STORAGE_KEY);
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
