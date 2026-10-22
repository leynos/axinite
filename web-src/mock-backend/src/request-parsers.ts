/** @file Runtime parsers for JSON request bodies received by the mock gateway. */

import type {
  ApprovalRequest,
  AuthCancelRequest,
  AuthTokenRequest,
  ExtensionSetupRequest,
  ImageData,
  JobPromptRequest,
  MemorySearchRequest,
  MemoryWriteRequest,
  PairingApproveRequest,
  SendMessageRequest,
  SkillInstallRequest,
  SkillSearchRequest,
  ToggleRequest,
} from "../../axinite/src/lib/api/contracts";
import {
  isBoolean,
  isJsonObject,
  isString,
  type JsonObject,
} from "../../axinite/src/lib/api/json-value";

function hasOptionalString(
  body: JsonObject,
  key: string,
  nullable = false
): boolean {
  const value = body[key];
  return (
    value === undefined ||
    isString(value) ||
    (nullable && value === null)
  );
}

function isStringRecord(value: unknown): value is Record<string, string> {
  return (
    isJsonObject(value) && Object.values(value).every((entry) => isString(entry))
  );
}

function isImageData(value: unknown): value is ImageData {
  return (
    isJsonObject(value) &&
    isString(value["media_type"]) &&
    isString(value["data"])
  );
}

export function isSendMessageRequest(
  value: unknown
): value is SendMessageRequest {
  return (
    isJsonObject(value) &&
    isString(value["content"]) &&
    Array.isArray(value["images"]) &&
    value["images"].every(isImageData) &&
    hasOptionalString(value, "thread_id", true) &&
    hasOptionalString(value, "timezone", true)
  );
}

export function isApprovalRequest(value: unknown): value is ApprovalRequest {
  return (
    isJsonObject(value) &&
    isString(value["request_id"]) &&
    isString(value["action"]) &&
    hasOptionalString(value, "thread_id", true)
  );
}

export function isAuthTokenRequest(value: unknown): value is AuthTokenRequest {
  return (
    isJsonObject(value) &&
    isString(value["extension_name"]) &&
    isString(value["token"])
  );
}

export function isAuthCancelRequest(
  value: unknown
): value is AuthCancelRequest {
  return isJsonObject(value) && isString(value["extension_name"]);
}

export function isPairingApproveRequest(
  value: unknown
): value is PairingApproveRequest {
  return isJsonObject(value) && isString(value["code"]);
}

export function isMemoryWriteRequest(
  value: unknown
): value is MemoryWriteRequest {
  return (
    isJsonObject(value) &&
    isString(value["path"]) &&
    isString(value["content"])
  );
}

export function isMemorySearchRequest(
  value: unknown
): value is MemorySearchRequest {
  if (!isJsonObject(value) || !isString(value["query"])) return false;
  const { limit } = value;
  return limit === undefined || (typeof limit === "number" && Number.isFinite(limit));
}

export function isJobPromptRequest(value: unknown): value is JobPromptRequest {
  return (
    isJsonObject(value) &&
    isString(value["content"]) &&
    (value["done"] === undefined || isBoolean(value["done"]))
  );
}

export function isToggleRequest(value: unknown): value is ToggleRequest {
  return (
    isJsonObject(value) &&
    (value["enabled"] === undefined || isBoolean(value["enabled"]))
  );
}

export function isExtensionSetupRequest(
  value: unknown
): value is ExtensionSetupRequest {
  return isJsonObject(value) && isStringRecord(value["secrets"]);
}

export function isSkillSearchRequest(
  value: unknown
): value is SkillSearchRequest {
  return isJsonObject(value) && isString(value["query"]);
}

export function isSkillInstallRequest(
  value: unknown
): value is SkillInstallRequest {
  return (
    isJsonObject(value) &&
    isString(value["name"]) &&
    hasOptionalString(value, "slug", true) &&
    hasOptionalString(value, "url", true) &&
    hasOptionalString(value, "content", true)
  );
}

export async function parseJsonBody<T>(
  request: Request,
  isBody: (value: unknown) => value is T
): Promise<T | undefined> {
  try {
    const value: unknown = await request.json();
    return isBody(value) ? value : undefined;
  } catch {
    return undefined;
  }
}
