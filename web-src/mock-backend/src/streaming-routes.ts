const STREAMING_API_PATHS = new Set(["/api/chat/events", "/api/logs/events"]);

export function isStreamingApiPath(pathname: string): boolean {
  return STREAMING_API_PATHS.has(pathname);
}
