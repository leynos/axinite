/**
 * Registry of the long-lived SSE API paths. The mock API server and the
 * preview proxy use it to disable the request idle timeout for these routes.
 */

const STREAMING_API_PATHS =new Set(["/api/chat/events", "/api/logs/events"]);

export function isStreamingApiPath(pathname: string): boolean {
  return STREAMING_API_PATHS.has(pathname);
}
