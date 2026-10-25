//! Generated invariants for recursive JSON and representative gateway payloads.

import fc from "fast-check";
import { describe, expect, it } from "vitest";
import { isChatSseEvent } from "@/lib/api/chat-event-parser";
import { isJsonObject, isJsonValue } from "@/lib/api/json-value";
import { isSendMessageRequest } from "../../mock-backend/src/request-parsers";

const jsonValues = fc.jsonValue({ maxDepth: 4 });
const identifiers = fc.string({ maxLength: 40 });

describe("JSON boundary properties", () => {
  it("accepts generated JSON values and their text round trips", () => {
    fc.assert(
      fc.property(jsonValues, (value) => {
        expect(isJsonValue(value)).toBe(true);
        const encoded = JSON.stringify(value);
        const decoded: unknown = JSON.parse(encoded);
        expect(isJsonValue(decoded)).toBe(true);
        expect(JSON.stringify(value)).toBe(encoded);
      })
    );
  });

  it("rejects invalid leaves even when nested in otherwise valid JSON", () => {
    fc.assert(
      fc.property(
        jsonValues,
        fc.constantFrom(
          undefined,
          Number.NaN,
          Number.POSITIVE_INFINITY,
          Number.NEGATIVE_INFINITY
        ),
        (valid, invalid) => {
          expect(isJsonValue({ valid, nested: [{ invalid }] })).toBe(false);
        }
      )
    );
  });

  it("rejects class instances regardless of their JSON-shaped fields", () => {
    class WrappedValue<T> {
      constructor(readonly value: T) {}
    }
    fc.assert(
      fc.property(jsonValues, (value) => {
        expect(isJsonObject(new WrappedValue(value))).toBe(false);
      })
    );
  });

  it("preserves arbitrary JSON inputs on job tool events", () => {
    fc.assert(
      fc.property(
        identifiers,
        identifiers,
        jsonValues,
        (jobId, toolName, input) => {
          const event = {
            type: "job_tool_use",
            job_id: jobId,
            tool_name: toolName,
            input,
          };
          expect(isChatSseEvent(event)).toBe(true);
          expect(
            isChatSseEvent({ ...event, input: { invalid: undefined } })
          ).toBe(false);
        }
      )
    );
  });

  it("accepts generated chat requests and rejects a wrong nested image field", () => {
    fc.assert(
      fc.property(identifiers, identifiers, (content, threadId) => {
        const request = { content, thread_id: threadId, images: [] };
        expect(isSendMessageRequest(request)).toBe(true);
        expect(
          isSendMessageRequest({
            ...request,
            images: [{ data: content, media_type: 42 }],
          })
        ).toBe(false);
      })
    );
  });
});
