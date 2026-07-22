import { createSignal } from "solid-js";

/**
 * Global chat-stream connection indicator, shared between the chat surface
 * (which owns the EventSource lifecycle) and the app shell (which renders the
 * status dot). It is a module-level Solid signal so any component can read the
 * current state reactively without prop-drilling through the router.
 *
 * States:
 * - `idle`: no chat stream has been opened yet (the shell's default before the
 *   operator visits the chat route). Rendering "disconnected" here would be
 *   misleading, so the neutral `idle` state is used instead.
 * - `connecting`: a stream open has been requested but the browser has not yet
 *   confirmed the connection (EventSource has not fired `open`).
 * - `connected`: the EventSource `open` event has fired.
 * - `disconnected`: the stream errored or was deliberately closed.
 *
 * The Playwright e2e suite targets the `connected` and `disconnected` values
 * via the shell's `data-state` attribute, so those spellings are load-bearing.
 */
export type ConnectionState =
  | "idle"
  | "connecting"
  | "connected"
  | "disconnected";

const [connectionState, setConnectionStateSignal] =
  createSignal<ConnectionState>("idle");

export { connectionState };

export function setConnectionState(state: ConnectionState): void {
  setConnectionStateSignal(state);
}

export function markConnecting(): void {
  setConnectionStateSignal("connecting");
}

export function markConnected(): void {
  setConnectionStateSignal("connected");
}

export function markDisconnected(): void {
  setConnectionStateSignal("disconnected");
}
