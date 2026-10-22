//! Decodes gateway chat events before they enter the typed event pipeline.

import type { ChatSseEvent } from "@/lib/api/contracts";
import {
  isBoolean,
  isJsonObject,
  isJsonValue,
  isString,
  type JsonObject,
  type JsonValue,
} from "@/lib/api/json-value";

function hasStrings(value: JsonObject, fields: readonly string[]): boolean {
  return fields.every((field) => isString(value[field]));
}

function hasOptionalStrings(
  value: JsonObject,
  fields: readonly string[]
): boolean {
  return fields.every((field) => {
    const fieldValue = value[field];
    return fieldValue === undefined || isString(fieldValue);
  });
}

/** Validates every gateway chat event before it reaches application code. */
export function isChatSseEvent(value: JsonValue): value is ChatSseEvent {
  if (!isJsonObject(value) || !isString(value["type"])) {
    return false;
  }

  switch (value["type"]) {
    case "response": {
      return hasStrings(value, ["content", "thread_id"]);
    }
    case "thinking":
    case "status":
    case "error": {
      return (
        hasStrings(value, ["message"]) &&
        hasOptionalStrings(value, ["thread_id"])
      );
    }
    case "tool_started": {
      return (
        hasStrings(value, ["name"]) && hasOptionalStrings(value, ["thread_id"])
      );
    }
    case "tool_completed": {
      return (
        hasStrings(value, ["name"]) &&
        isBoolean(value["success"]) &&
        hasOptionalStrings(value, ["error", "parameters", "thread_id"])
      );
    }
    case "tool_result": {
      return (
        hasStrings(value, ["name", "preview"]) &&
        hasOptionalStrings(value, ["thread_id"])
      );
    }
    case "stream_chunk": {
      return (
        hasStrings(value, ["content"]) &&
        hasOptionalStrings(value, ["thread_id"])
      );
    }
    case "approval_needed": {
      return (
        hasStrings(value, [
          "request_id",
          "tool_name",
          "description",
          "parameters",
        ]) && hasOptionalStrings(value, ["thread_id"])
      );
    }
    case "auth_required": {
      return (
        hasStrings(value, ["extension_name"]) &&
        hasOptionalStrings(value, ["instructions", "auth_url", "setup_url"])
      );
    }
    case "auth_completed": {
      return (
        hasStrings(value, ["extension_name", "message"]) &&
        isBoolean(value["success"])
      );
    }
    case "heartbeat": {
      return true;
    }
    case "extension_status": {
      return (
        hasStrings(value, ["extension_name", "status"]) &&
        hasOptionalStrings(value, ["message"])
      );
    }
    case "job_started": {
      return hasStrings(value, ["job_id", "title", "browse_url"]);
    }
    case "job_message": {
      return hasStrings(value, ["job_id", "role", "content"]);
    }
    case "job_tool_use": {
      return (
        hasStrings(value, ["job_id", "tool_name"]) &&
        isJsonValue(value["input"])
      );
    }
    case "job_tool_result": {
      return hasStrings(value, ["job_id", "tool_name", "output"]);
    }
    case "job_status": {
      return hasStrings(value, ["job_id", "message"]);
    }
    case "job_result": {
      return (
        hasStrings(value, ["job_id", "status"]) &&
        hasOptionalStrings(value, ["session_id"])
      );
    }
    case "image_generated": {
      return (
        hasStrings(value, ["data_url"]) &&
        hasOptionalStrings(value, ["path", "thread_id"])
      );
    }
    default: {
      return false;
    }
  }
}
