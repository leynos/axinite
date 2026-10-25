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

type RouteDefinition = {
  handler: RouteHandler;
  method: string;
  path: RegExp | string;
};

type RouteRequest = {
  method: string;
  path: string;
};

type JsonResponseDefinition = RouteRequest & {
  payload: JsonValue;
  status?: number;
};

type ErrorResponseDefinition = RouteRequest & {
  message: string;
  status: number;
};

type JsonEventFrame = {
  payload: JsonValue;
  type: string;
};

type RawEventFrame = {
  data: string;
  type: string;
};

type RequestBodyResult =
  | { isValid: false }
  | { body?: JsonObject; isValid: true };

async function readRequestBody(request: Request): Promise<RequestBodyResult> {
  if (!request.body) {
    return { isValid: true };
  }

  let parsed: unknown;
  try {
    parsed = await request.clone().json();
  } catch {
    return { isValid: false };
  }
  if (!isJsonObject(parsed)) {
    return { isValid: false };
  }

  return { isValid: true, body: parsed };
}

function routeMatches(route: RouteDefinition, request: RouteRequest): boolean {
  if (route.method !== request.method) {
    return false;
  }

  if (route.path instanceof RegExp) {
    return route.path.test(request.path);
  }

  return route.path === request.path;
}

function findMatchingRoute(
  routes: readonly RouteDefinition[],
  request: RouteRequest
): RouteDefinition | undefined {
  return routes.toReversed().find((route) => routeMatches(route, request));
}

function unmatchedRouteResponse(request: RouteRequest): Response {
  return Response.json(
    { error: `No test response for ${request.method} ${request.path}` },
    { status: 404 }
  );
}

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
  emit(frame: JsonEventFrame): void {
    this.frame({ type: frame.type, data: JSON.stringify(frame.payload) });
  }

  /** Deliver an SSE frame with raw data for malformed-payload tests. */
  frame(frame: RawEventFrame): void {
    this.dispatchEvent(new MessageEvent(frame.type, { data: frame.data }));
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
}

/** Mutable gateway responses plus an observable request log for one test. */
export class GatewayTestServer {
  readonly requests: RecordedRequest[] = [];
  private routes: RouteDefinition[] = [];

  constructor() {
    GatewayEventSource.latest = undefined;
    vi.stubGlobal("EventSource", GatewayEventSource);
    setApiRequestHandler((request) => this.handle(request));
  }

  /** Return one JSON response for a method and exact path. */
  respond(definition: JsonResponseDefinition): this {
    const { method, path, payload, status = 200 } = definition;
    return this.route({
      method,
      path,
      handler: async () => Response.json(payload, { status }),
    });
  }

  /** Return a plain-text HTTP error for a method and exact path. */
  fail(definition: ErrorResponseDefinition): this {
    const { method, path, status, message } = definition;
    return this.route({
      method,
      path,
      handler: async () => new Response(message, { status }),
    });
  }

  /** Register a custom response handler; later matching routes take priority. */
  route(definition: RouteDefinition): this {
    this.routes.push({
      ...definition,
      method: definition.method.toUpperCase(),
    });
    return this;
  }

  private async handle(request: Request): Promise<Response> {
    const url = new URL(request.url);
    const method = request.method.toUpperCase();
    const bodyResult = await readRequestBody(request);
    if (!bodyResult.isValid) {
      return new Response("Request body was not JSON", { status: 400 });
    }

    const routeRequest: RouteRequest = {
      method,
      path: url.pathname,
    };
    const recordedRequest: RecordedRequest = {
      ...routeRequest,
      search: url.search,
    };
    if (bodyResult.body) recordedRequest.body = bodyResult.body;
    this.requests.push(recordedRequest);

    const route = findMatchingRoute(this.routes, routeRequest);
    return route
      ? route.handler(request)
      : unmatchedRouteResponse(routeRequest);
  }
}
