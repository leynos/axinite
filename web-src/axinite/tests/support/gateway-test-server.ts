/**
 * @file In-process HTTP and SSE seams for front-end behaviour tests. The tests
 * call the real API modules and control only the gateway responses.
 */

import { vi } from "vitest";

import {
  isJsonObject,
  type JsonObject,
  type JsonValue,
} from "@/lib/api/json-value";
import { setApiRequestHandler } from "./i18n-test-runtime";

type RecordedRequest = {
  body?: JsonObject;
  method: string;
  path: string;
  search: string;
};

type RouteHandler = (request: Request) => Promise<Response>;

type Route = {
  handler: RouteHandler;
  method: string;
  path: RegExp | string;
};

/** EventSource implementation whose server frames are emitted by each test. */
export class GatewayEventSource extends EventTarget {
  static latest: GatewayEventSource | undefined;

  readonly CONNECTING = 0;
  readonly OPEN = 1;
  readonly CLOSED = 2;
  readonly url: string;
  readonly withCredentials = false;
  readyState = 0;
  onerror: ((this: GatewayEventSource, event: Event) => void) | null = null;
  onmessage:
    | ((this: GatewayEventSource, event: MessageEvent<string>) => void)
    | null = null;
  onopen: ((this: GatewayEventSource, event: Event) => void) | null = null;
  close = vi.fn(() => {
    this.readyState = 2;
  });

  constructor(url: string) {
    super();
    this.url = url;
    GatewayEventSource.latest = this;
  }

  /** Deliver a JSON SSE frame through the same listeners used by browsers. */
  emit(type: string, payload: JsonValue): void {
    this.frame(type, JSON.stringify(payload));
  }

  /** Deliver an SSE frame with raw data for malformed-payload tests. */
  frame(type: string, data: string): void {
    this.dispatchEvent(new MessageEvent(type, { data }));
  }

  /** Deliver the native EventSource open event. */
  open(): void {
    this.readyState = 1;
    this.dispatchEvent(new Event("open"));
  }

  /** Deliver a native transport error without an SSE data payload. */
  fail(): void {
    this.dispatchEvent(new Event("error"));
  }

  override dispatchEvent(event: Event): boolean {
    const dispatched = super.dispatchEvent(event);
    if (event.type === "open") this.onopen?.call(this, event);
    if (event.type === "error") this.onerror?.call(this, event);
    if (event.type === "message" && event instanceof MessageEvent) {
      this.onmessage?.call(this, event);
    }
    return dispatched;
  }
}

/** Mutable gateway responses plus an observable request log for one test. */
export class GatewayTestServer {
  readonly requests: RecordedRequest[] = [];
  private routes: Route[] = [];

  constructor() {
    GatewayEventSource.latest = undefined;
    vi.stubGlobal("EventSource", GatewayEventSource);
    setApiRequestHandler((request) => this.handle(request));
  }

  /** Return one JSON response for a method and exact path. */
  respond(
    method: string,
    path: string,
    payload: JsonValue,
    status = 200
  ): this {
    return this.route(method, path, async () =>
      Response.json(payload, { status })
    );
  }

  /** Return a plain-text HTTP error for a method and exact path. */
  fail(method: string, path: string, status: number, message: string): this {
    return this.route(
      method,
      path,
      async () => new Response(message, { status })
    );
  }

  /** Register a custom response handler; later matching routes take priority. */
  route(method: string, path: RegExp | string, handler: RouteHandler): this {
    this.routes.push({ handler, method: method.toUpperCase(), path });
    return this;
  }

  private async handle(request: Request): Promise<Response> {
    const url = new URL(request.url);
    const method = request.method.toUpperCase();
    let body: JsonObject | undefined;

    if (request.body) {
      const parsed: unknown = await request.clone().json();
      if (!isJsonObject(parsed)) {
        return new Response("Request body was not JSON", { status: 400 });
      }
      body = parsed;
    }

    const recordedRequest: RecordedRequest = {
      method,
      path: url.pathname,
      search: url.search,
    };
    if (body) recordedRequest.body = body;
    this.requests.push(recordedRequest);

    for (const route of this.routes.toReversed()) {
      const pathMatches =
        route.path instanceof RegExp
          ? route.path.test(url.pathname)
          : route.path === url.pathname;
      if (route.method === method && pathMatches) {
        return route.handler(request);
      }
    }

    return Response.json(
      { error: `No test response for ${method} ${url.pathname}` },
      { status: 404 }
    );
  }
}
