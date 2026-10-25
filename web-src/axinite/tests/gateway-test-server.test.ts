/** @file Contract tests for the in-process gateway test server. */

import { afterEach, describe, expect, it } from "vitest";
import { GatewayTestServer } from "./support/gateway-test-server";
import { setApiRequestHandler } from "./support/i18n-test-runtime";

afterEach(() => setApiRequestHandler(undefined));

describe("gateway test server request parsing", () => {
  it("returns HTTP 400 for malformed JSON request bodies", async () => {
    const gateway = new GatewayTestServer().respond({
      method: "POST",
      path: "/api/example",
      payload: { accepted: true },
    });

    const response = await fetch("/api/example", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: "{ malformed",
    });

    expect(response.status).toBe(400);
    expect(await response.text()).toBe("Request body was not JSON");
    expect(gateway.requests).toEqual([]);
  });
});
