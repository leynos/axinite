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

/** Validates the recursive value set produced by parsing JSON text. */
export function isJsonValue(value: unknown): value is JsonValue {
  if (value === null || isString(value) || isBoolean(value)) {
    return true;
  }

  if (typeof value === "number") {
    return Number.isFinite(value);
  }

  if (Array.isArray(value)) {
    return value.every((entry) => isJsonValue(entry));
  }

  if (!(value instanceof Object)) {
    return false;
  }

  const prototype = Object.getPrototypeOf(value);
  return (
    (prototype === Object.prototype || prototype === null) &&
    Object.values(value).every((entry) => isJsonValue(entry))
  );
}

/** Validates an object payload and every value it contains. */
export function isJsonObject(value: unknown): value is JsonObject {
  if (!(value instanceof Object) || Array.isArray(value)) {
    return false;
  }

  const prototype = Object.getPrototypeOf(value);
  return (
    (prototype === Object.prototype || prototype === null) &&
    Object.values(value).every((entry) => isJsonValue(entry))
  );
}
