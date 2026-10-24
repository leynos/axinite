//! Decodes gateway chat events before they enter the typed event pipeline.

import type { ChatSseEvent } from "@/lib/api/contracts";
import {
  isBoolean,
  isJsonObject,
  isString,
  type JsonObject,
} from "@/lib/api/json-value";

type ChatEventType = ChatSseEvent["type"];

type ChatEventFields = {
  requiredStrings?: readonly string[];
  optionalStrings?: readonly string[];
  requiredBooleans?: readonly string[];
  requiredValues?: readonly string[];
};

const CHAT_EVENT_FIELDS = {
  response: { requiredStrings: ["content", "thread_id"] },
  thinking: {
    requiredStrings: ["message"],
    optionalStrings: ["thread_id"],
  },
  tool_started: { requiredStrings: ["name"], optionalStrings: ["thread_id"] },
  tool_completed: {
    requiredStrings: ["name"],
    optionalStrings: ["error", "parameters", "thread_id"],
    requiredBooleans: ["success"],
  },
  tool_result: {
    requiredStrings: ["name", "preview"],
    optionalStrings: ["thread_id"],
  },
  stream_chunk: {
    requiredStrings: ["content"],
    optionalStrings: ["thread_id"],
  },
  status: { requiredStrings: ["message"], optionalStrings: ["thread_id"] },
  approval_needed: {
    requiredStrings: ["request_id", "tool_name", "description", "parameters"],
    optionalStrings: ["thread_id"],
  },
  auth_required: {
    requiredStrings: ["extension_name"],
    optionalStrings: ["instructions", "auth_url", "setup_url"],
  },
  auth_completed: {
    requiredStrings: ["extension_name", "message"],
    requiredBooleans: ["success"],
  },
  error: { requiredStrings: ["message"], optionalStrings: ["thread_id"] },
  heartbeat: {},
  extension_status: {
    requiredStrings: ["extension_name", "status"],
    optionalStrings: ["message"],
  },
  job_started: {
    requiredStrings: ["job_id", "title", "browse_url"],
  },
  job_message: { requiredStrings: ["job_id", "role", "content"] },
  job_tool_use: {
    requiredStrings: ["job_id", "tool_name"],
    requiredValues: ["input"],
  },
  job_tool_result: {
    requiredStrings: ["job_id", "tool_name", "output"],
  },
  job_status: { requiredStrings: ["job_id", "message"] },
  job_result: {
    requiredStrings: ["job_id", "status"],
    optionalStrings: ["session_id"],
  },
  image_generated: {
    requiredStrings: ["data_url"],
    optionalStrings: ["path", "thread_id"],
  },
} satisfies Record<ChatEventType, ChatEventFields>;

function hasStrings(value: JsonObject, fields: readonly string[]): boolean {
  return fields.every((field) => isString(value[field]));
}

function hasOptionalStrings(
  value: JsonObject,
  fields: readonly string[]
): boolean {
  for (const field of fields) {
    const fieldValue = value[field];
    if (fieldValue === undefined) {
      continue;
    }
    if (!isString(fieldValue)) {
      return false;
    }
  }
  return true;
}

function hasBooleans(value: JsonObject, fields: readonly string[]): boolean {
  return fields.every((field) => isBoolean(value[field]));
}

function hasRequiredValues(
  value: JsonObject,
  fields: readonly string[]
): boolean {
  return fields.every((field) => value[field] !== undefined);
}

function hasEventFields(value: JsonObject, fields: ChatEventFields): boolean {
  if (!hasStrings(value, fields.requiredStrings ?? [])) {
    return false;
  }
  if (!hasOptionalStrings(value, fields.optionalStrings ?? [])) {
    return false;
  }
  if (!hasBooleans(value, fields.requiredBooleans ?? [])) {
    return false;
  }
  return hasRequiredValues(value, fields.requiredValues ?? []);
}

function isChatEventType(value: string): value is ChatEventType {
  return Object.hasOwn(CHAT_EVENT_FIELDS, value);
}

/** Validates every gateway chat event before it reaches application code. */
export function isChatSseEvent(value: unknown): value is ChatSseEvent {
  if (!isJsonObject(value)) {
    return false;
  }
  const eventType = value["type"];
  if (!isString(eventType)) {
    return false;
  }
  if (!isChatEventType(eventType)) {
    return false;
  }
  return hasEventFields(value, CHAT_EVENT_FIELDS[eventType]);
}
