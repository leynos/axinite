import type { ChatSseEvent } from "@/lib/api/contracts";

/**
 * Deliberate, minimal test-hook surface for the Python Playwright e2e suite.
 *
 * The compiled SPA exposes no reachable module globals, yet the e2e scenarios
 * need to drive the chat stream deterministically (open, close, reconnect, and
 * inject synthetic gateway events) without a live daemon push. Rather than
 * recreate the legacy UI's implicit global soup, this module mounts a single
 * documented object at `window.__axinite`. It adds no capability an open
 * browser console does not already have.
 *
 * The chat surface registers the concrete stream controls on mount and
 * unregisters them on cleanup, so every hook is a safe no-op whenever the chat
 * route is not mounted.
 */
export type ChatStreamHooks = {
  /** Close the active chat EventSource and mark the connection disconnected. */
  close: () => void;
  /** Re-open the chat EventSource, re-registering its listeners. */
  reconnect: () => void;
  /** Feed a synthetic event into the chat stream handler. */
  emit: (event: ChatSseEvent) => void;
};

export type AxiniteTestHooks = {
  version: 1;
  closeChatStream: () => void;
  reconnectChatStream: () => void;
  emitChatEvent: (event: ChatSseEvent) => void;
};

let registered: ChatStreamHooks | null = null;

export function registerChatStreamHooks(hooks: ChatStreamHooks): void {
  registered = hooks;
}

export function unregisterChatStreamHooks(hooks: ChatStreamHooks): void {
  // Only clear the slot if the caller still owns it; a remount may have already
  // installed a fresh set of controls.
  if (registered === hooks) {
    registered = null;
  }
}

/**
 * Installs `window.__axinite`. Idempotent and safe to call at every boot: it
 * always rebinds to the currently registered chat-stream controls, so hooks
 * follow the chat route's mount lifecycle rather than capturing a stale set.
 */
export function installTestHooks(): void {
  if (typeof window === "undefined") {
    return;
  }
  const hooks: AxiniteTestHooks = {
    version: 1,
    closeChatStream: () => registered?.close(),
    reconnectChatStream: () => registered?.reconnect(),
    emitChatEvent: (event) => registered?.emit(event),
  };
  window.__axinite = hooks;
}
