import { afterEach, describe, expect, it } from "vitest";

import { GatewayEventSource } from "./support/gateway-test-server";

type EventHandler = (event: Event) => void;

type EventDispatchCase = {
  dispatch: (source: GatewayEventSource) => void;
  property: "onerror" | "onmessage" | "onopen";
  type: "error" | "message" | "open";
};

const eventDispatchCases: readonly EventDispatchCase[] = [
  {
    type: "open",
    property: "onopen",
    dispatch: (source) => source.open(),
  },
  {
    type: "error",
    property: "onerror",
    dispatch: (source) => source.fail(),
  },
  {
    type: "message",
    property: "onmessage",
    dispatch: (source) =>
      source.dispatchEvent(new MessageEvent<string>("message", { data: "{}" })),
  },
];

function installPropertyHandler(
  source: GatewayEventSource,
  property: EventDispatchCase["property"],
  handler: EventHandler
): void {
  // Exercise the EventSource property-handler API; application listeners use addEventListener.
  Object.defineProperty(source, property, {
    configurable: true,
    value: handler,
    writable: true,
  });
}

describe("GatewayEventSource", () => {
  afterEach(() => {
    GatewayEventSource.latest = undefined;
  });

  it.each(eventDispatchCases)("calls both $type handlers exactly once", ({
    type,
    property,
    dispatch,
  }) => {
    const source = new GatewayEventSource("http://localhost/events");
    const propertyEvents: Event[] = [];
    const listenerEvents: Event[] = [];

    installPropertyHandler(source, property, (event) => {
      propertyEvents.push(event);
    });
    source.addEventListener(type, (event) => {
      listenerEvents.push(event);
    });

    dispatch(source);

    expect(propertyEvents).toHaveLength(1);
    expect(propertyEvents[0]?.type).toBe(type);
    expect(listenerEvents).toHaveLength(1);
    expect(listenerEvents[0]?.type).toBe(type);
    expect(listenerEvents[0]).toBe(propertyEvents[0]);
  });
});
