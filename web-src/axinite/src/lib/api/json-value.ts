//! Runtime predicates for values decoded from JSON boundaries.

/** A value that can be represented by JSON without losing its shape. */
export type JsonValue =
  | null
  | boolean
  | number
  | string
  | JsonValue[]
  | JsonObject;

/** A JSON object whose values retain their JSON contracts. */
export type JsonObject = { [key: string]: JsonValue };

/** Narrows strings after values cross an untyped I/O boundary. */
export function isString(value: unknown): value is string {
  return typeof value === "string";
}

/** Narrows booleans without accepting truthy values. */
export function isBoolean(value: unknown): value is boolean {
  return value === true || value === false;
}

function isJsonPrimitive(
  value: unknown
): value is null | boolean | number | string {
  switch (typeof value) {
    case "string":
    case "boolean": {
      return true;
    }
    case "number": {
      return Number.isFinite(value);
    }
    case "object": {
      return value === null;
    }
    default: {
      return false;
    }
  }
}

function isPlainRecord(value: unknown): value is object {
  if (value === null) {
    return false;
  }
  if (typeof value !== "object") {
    return false;
  }

  const prototype = Object.getPrototypeOf(value);
  if (prototype === null) {
    return true;
  }
  return prototype === Object.prototype;
}

/** Validates the recursive value set produced by parsing JSON text. */
export function isJsonValue(value: unknown): value is JsonValue {
  if (isJsonPrimitive(value)) {
    return true;
  }
  if (Array.isArray(value)) {
    return value.every((entry) => isJsonValue(entry));
  }
  return isJsonObject(value);
}

/** Validates an object payload and every value it contains. */
export function isJsonObject(value: unknown): value is JsonObject {
  if (!isPlainRecord(value)) {
    return false;
  }
  return Object.values(value).every((entry) => isJsonValue(entry));
}
