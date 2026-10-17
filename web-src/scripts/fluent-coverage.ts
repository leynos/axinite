/** @file Pure coverage keys for Fluent values and independently translated attributes. */

import { parse } from "@fluent/syntax";

/**
 * Collect separately addressable Fluent values and attributes. Terms retain
 * their leading hyphen, so a message cannot satisfy a missing term. An
 * attribute-only entry does not claim to supply a value.
 */
export function fluentCoverageKeys(source: string): Set<string> {
  const keys = new Set<string>();
  for (const entry of parse(source, {}).body) {
    if (entry.type !== "Message" && entry.type !== "Term") {
      continue;
    }
    const name = `${entry.type === "Term" ? "-" : ""}${entry.id.name}`;
    if (entry.value !== null) {
      keys.add(name);
    }
    for (const attribute of entry.attributes) {
      keys.add(`${name}.${attribute.id.name}`);
    }
  }
  return keys;
}
