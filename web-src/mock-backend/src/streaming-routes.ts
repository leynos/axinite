/**
 * @file Registry of the long-lived SSE API paths. The mock API server and the
 * preview proxy use it to disable the request idle timeout for these routes.
 */

const STREAMING_API_PATHS =new Set(["/api/chat/events", "/api/logs/events"]);

/**
 * Reports whether the pathname is exactly the chat-events or log-events
 * path; query strings are not part of the argument.
 */
export function isStreamingApiPath(pathname: string): boolean {
  return STREAMING_API_PATHS.has(pathname);
}
