/** @file Observe build readiness and forward subprocess output. */
import { stripVTControlCharacters } from "node:util";

/** Vite build completion signal and its output observer. */
export type BuildReadiness = {
  ready: Promise<void>;
  observeLine: (line: string) => void;
};

/** Observe Vite's completed build message before making the preview available. */
export function createBuildReadiness(): BuildReadiness {
  const events = new EventTarget();
  const ready = new Promise<void>((resolve) => {
    events.addEventListener("ready", () => resolve(), { once: true });
  });
  return {
    ready,
    observeLine: (line) => {
      // Vite emits this only after it has written the complete initial bundle.
      if (
        /^built in \d+(?:\.\d+)?(?:ms|s)\.$/.test(
          stripVTControlCharacters(line)
        )
      )
        events.dispatchEvent(new Event("ready"));
    },
  };
}

/** Forward child output and report complete lines to an optional observer. */
export function pipeOutput(
  label: string,
  stream: ReadableStream<Uint8Array> | null,
  writer: (line: string) => void,
  observeLine?: (line: string) => void
): void {
  if (!stream) return;
  const reader = stream.getReader();
  const decoder = new TextDecoder();
  let pending = "";
  void (async () => {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) {
        if (pending.length > 0) writer(`[${label}] ${pending}`);
        break;
      }
      pending += decoder.decode(value, { stream: true });
      const lines = pending.split(/\r?\n/);
      pending = lines.pop() ?? "";
      for (const line of lines) {
        writer(`[${label}] ${line}`);
        observeLine?.(line);
      }
    }
  })();
}
