//! Bounded, privacy-safe diagnostics for rejected gateway response boundaries.

type BoundaryOperation =
  | "chat-stream"
  | "log-stream"
  | "feature-flags"
  | "tee-status"
  | "tee-report";
type BoundaryReason = "invalid-json" | "invalid-payload" | "connection";

const counts = new Map<string, number>();

/** Read the page-local count for one fixed failure category. */
export function boundaryFailureCount(
  operation: BoundaryOperation,
  reason: BoundaryReason
): number {
  return counts.get(`${operation}:${reason}`) ?? 0;
}

/** Count every rejection, logging its fixed category once to avoid log floods. */
export function recordBoundaryFailure(
  operation: BoundaryOperation,
  reason: BoundaryReason
): void {
  const count = Math.min(
    boundaryFailureCount(operation, reason) + 1,
    Number.MAX_SAFE_INTEGER
  );
  counts.set(`${operation}:${reason}`, count);
  if (count === 1) {
    console.warn("[gateway-boundary] response failure", { operation, reason });
  }
}
