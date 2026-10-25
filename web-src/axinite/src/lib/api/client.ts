/**
 * @file Shared HTTP layer for the `lib/api/*` modules. It adds JSON helpers, the
 * bearer token, a request timeout, and EventSource creation with the token
 * passed as a query parameter. Non-2xx responses throw with the body text.
 */

import { appendTokenToUrl, getGatewayToken } from "@/lib/auth/token";

type RequestOptions = {
  headers?: HeadersInit;
  method?: string;
  body?: BodyInit | null;
};

/** How long any browser request may take before it is aborted. */
export const REQUEST_TIMEOUT_MS = 5000;

/** Bounded failure classes consumed by operator controls and safe diagnostics. */
export type RequestFailureCategory = "timeout" | "transport" | "http";

/** A failed gateway request with a semantic category and optional HTTP status. */
export class RequestError extends Error {
  /** Preserve the UI message separately from the bounded diagnostic category. */
  constructor(
    readonly category: RequestFailureCategory,
    message = "Gateway request failed",
    readonly status?: number
  ) {
    super(message);
    this.name = "RequestError";
  }
}

/**
 * Run `run` with an abort signal that fires after `timeoutMs`, so a hung
 * gateway cannot leave a caller waiting indefinitely. Rejects with a timeout
 * error when the deadline passes; other failures propagate unchanged.
 */
export async function withRequestTimeout<T>(
  run: (signal: AbortSignal) => Promise<T>,
  timeoutMs: number = REQUEST_TIMEOUT_MS
): Promise<T> {
  const controller = new AbortController();
  const timeoutId = setTimeout(() => controller.abort(), timeoutMs);
  try {
    return await run(controller.signal);
  } catch (error) {
    if (controller.signal.aborted) {
      throw new RequestError(
        "timeout",
        `Request timed out after ${timeoutMs}ms`
      );
    }
    throw error;
  } finally {
    clearTimeout(timeoutId);
  }
}

/**
 * Send a gateway request with JSON defaults and the session bearer token.
 * Explicit `Accept` and `Authorization` headers take precedence. Timeouts,
 * fetch failures, and non-2xx responses become categorized `RequestError`s;
 * HTTP errors retain the status and response text. A successful 204 resolves
 * to `undefined`; other successful responses are parsed as JSON.
 */
function request<T>(url: string, options: RequestOptions = {}): Promise<T> {
  return withRequestTimeout(async (signal) => {
    const token = getGatewayToken();
    const headers = new Headers(options.headers);
    if (!headers.has("Accept")) headers.set("Accept", "application/json");
    if (token && !headers.has("Authorization")) {
      headers.set("Authorization", `Bearer ${token}`);
    }
    const response = await fetch(url, {
      ...options,
      signal,
      headers,
    }).catch(() => {
      throw new RequestError("transport");
    });

    if (!response.ok) {
      const message = await response.text();
      throw new RequestError(
        "http",
        message.length > 0 ? message : `Request failed with ${response.status}`,
        response.status
      );
    }

    if (response.status === 204) {
      // SAFETY: The client contract treats a successful empty response as the
      // generic caller's empty result, as covered by the shared HTTP tests.
      return undefined as T;
    }

    // SAFETY: API wrappers bind T to the documented endpoint DTOs; the shared
    // HTTP layer owns transport decoding and endpoint contract tests cover them.
    return (await response.json()) as T;
  });
}

/** Normalize all HeadersInit forms while preserving explicit caller overrides. */
function jsonHeaders(additional?: HeadersInit): Headers {
  const headers = new Headers(additional);
  if (!headers.has("Content-Type"))
    headers.set("Content-Type", "application/json");
  return headers;
}

/** GET `url` with the bearer token and decode the JSON response. */
export function requestJson<T>(url: string): Promise<T> {
  return request<T>(url);
}

/** Assemble a JSON write while preserving the public verb-specific helpers. */
function writeJson<T, TBody extends object = never>(
  method: "POST" | "PUT",
  url: string,
  body?: TBody,
  headers?: HeadersInit
): Promise<T> {
  return request<T>(url, {
    method,
    headers: jsonHeaders(headers),
    body: body === undefined ? null : JSON.stringify(body),
  });
}

/** POST an optional JSON `body` to `url` and decode the JSON response. */
export function postJson<T, TBody extends object = never>(
  url: string,
  body?: TBody,
  headers?: HeadersInit
): Promise<T> {
  return writeJson<T, TBody>("POST", url, body, headers);
}

/** PUT an optional JSON `body` to `url` and decode the JSON response. */
export function putJson<T, TBody extends object = never>(
  url: string,
  body?: TBody,
  headers?: HeadersInit
): Promise<T> {
  return writeJson<T, TBody>("PUT", url, body, headers);
}

/** DELETE `url` and decode the JSON response. */
export function deleteJson<T>(url: string): Promise<T> {
  return request<T>(url, {
    method: "DELETE",
  });
}

/**
 * Report transport errors without confusing a server's named `error` message
 * with a connection failure. MessageEvent frames carry `data`, including
 * malformed payloads; transport error events do not.
 */
export function setStreamConnectionErrorHandler(
  source: EventSource,
  onError: () => void
): void {
  source.addEventListener("error", (event) => {
    if ("data" in event) return;
    onError();
  });
}

class InertEventSource extends EventTarget {
  readonly CONNECTING = 0;
  readonly OPEN = 1;
  readonly CLOSED = 2;
  readonly withCredentials = false;
  readonly readyState = 0;
  readonly url: string;
  onerror: ((this: InertEventSource, event: Event) => void) | null = null;
  onmessage:
    | ((this: InertEventSource, event: MessageEvent<string>) => void)
    | null = null;
  onopen: ((this: InertEventSource, event: Event) => void) | null = null;

  constructor(url: string) {
    super();
    this.url = url;
  }

  close(): void {}
}

/**
 * Open an EventSource for `url` with the token appended as a query parameter.
 * Outside a browser it returns an inert stub.
 */
export function createEventStream(url: string): EventSource {
  const streamUrl = appendTokenToUrl(url);
  if (typeof EventSource === "undefined") {
    // SAFETY: The inert target mirrors EventSource's fields and close method;
    // its inherited EventTarget listener overload is broader than the DOM API.
    return new InertEventSource(streamUrl) as EventSource;
  }

  return new EventSource(streamUrl, { withCredentials: false });
}
