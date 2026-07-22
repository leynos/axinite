const STORAGE_KEY = "axinite.gateway-token";

function storage(): Storage | null {
  if (typeof window === "undefined" || !window.sessionStorage) {
    return null;
  }
  return window.sessionStorage;
}

export function getGatewayToken(): string | null {
  return storage()?.getItem(STORAGE_KEY) ?? null;
}

export function setGatewayToken(token: string): void {
  storage()?.setItem(STORAGE_KEY, token);
}

export function clearGatewayToken(): void {
  storage()?.removeItem(STORAGE_KEY);
}

// EventSource cannot set request headers, so the gateway accepts the bearer
// token as a `token` query parameter on its streaming GET endpoints
// (src/channels/web/auth.rs::allows_query_token_auth).
export function appendTokenToUrl(url: string): string {
  const token = getGatewayToken();
  if (!token) {
    return url;
  }
  const separator = url.includes("?") ? "&" : "?";
  return `${url}${separator}token=${encodeURIComponent(token)}`;
}
