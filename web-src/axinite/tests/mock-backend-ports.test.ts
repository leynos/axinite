import { describe, expect, it } from "vitest";

import { parsePort } from "../../mock-backend/src/ports";

describe("parsePort", () => {
  it.each([
    [undefined, 8787],
    ["", 8787],
    ["  ", 8787],
    ["2020", 2020],
    [" 1 ", 1],
    ["65535", 65_535],
  ])("reads %j as %i", (raw, expected) => {
    expect(parsePort("MOCK_API_PORT", raw, 8787)).toBe(expected);
  });

  it.each([
    "abc",
    "80.5",
    "0",
    "65536",
    "-1",
    "8080x",
  ])("rejects %j with the variable name", (raw) => {
    expect(() => parsePort("PREVIEW_PORT", raw, 2020)).toThrow(
      /PREVIEW_PORT must be an integer TCP port/u
    );
  });
});
