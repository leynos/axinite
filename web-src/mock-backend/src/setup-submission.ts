/**
 * @file Setup submission application for the mock backend: the per-field
 * mutation the real handler performs, split out so `state.ts` stays a
 * readable size.
 */

import type {
  EndpointFieldInfo,
  EndpointOperations,
  SecretFieldInfo,
  SetupOutcomeInfo,
} from "../../axinite/src/lib/api/contracts";

import { isNonBlankString } from "./state-helpers";

/**
 * Apply submitted secret values, recording one outcome per declared secret.
 *
 * A blank value keeps the stored one, matching the real handler.
 */
export function applySecretSubmission(
  fields: SecretFieldInfo[],
  secrets: Record<string, string>,
  outcomes: SetupOutcomeInfo[]
): SecretFieldInfo[] {
  return fields.map((field) => {
    const written = isNonBlankString(secrets[field.name]);
    outcomes.push({
      field: field.name,
      kind: "secret",
      outcome: written ? "stored" : "kept",
    });
    return { ...field, provided: field.provided || written };
  });
}

/**
 * Apply submitted endpoint operations, recording one outcome per declared
 * endpoint. Clearing wins over a replacement for the same name.
 */
export function applyEndpointSubmission(
  fields: EndpointFieldInfo[],
  operations: EndpointOperations,
  outcomes: SetupOutcomeInfo[]
): EndpointFieldInfo[] {
  const set = operations.set ?? {};
  const clear = new Set(operations.clear);
  return fields.map((field) => {
    if (clear.has(field.name)) {
      outcomes.push({
        field: field.name,
        kind: "endpoint",
        outcome: "cleared",
      });
      return { ...field, state: "missing" as const };
    }
    if (isNonBlankString(set[field.name])) {
      outcomes.push({
        field: field.name,
        kind: "endpoint",
        outcome: "stored",
      });
      return { ...field, state: "configured" as const };
    }
    return field;
  });
}
