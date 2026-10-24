/**
 * @file Restart button and confirmation dialog for the app shell. The gateway has
 * no restart endpoint, so this wires `lib/restart` to the chat API (sending
 * `/restart`), the chat event stream, and gateway status polling.
 */

import { AlertDialog } from "@kobalte/core/alert-dialog";
import type { Component } from "solid-js";
import { createSignal, onCleanup, Show } from "solid-js";
import { connectChatEvents, sendMessage } from "@/lib/api/chat";
import {
  RequestError,
  setStreamConnectionErrorHandler,
} from "@/lib/api/client";
import { fetchGatewayStatusRaw } from "@/lib/api/gateway";
import { useI18n } from "@/lib/i18n/provider";
import {
  createRestartController,
  type RestartController,
  type RestartDeps,
  type RestartPhase,
} from "@/lib/restart";

const STATUS_POLL_MS = 3000;

type RestartControlProps = {
  restartEnabled: () => boolean | undefined;
  /** Injectable controller dependencies; defaults wire the live gateway. */
  makeDeps?: (onPhase: (phase: RestartPhase) => void) => RestartDeps;
};

/**
 * Bind controller ports to the live chat, event-stream, status, and timer APIs.
 * Restart command failures retain only their bounded category; the stream
 * handle and poll cancel function own cleanup of their browser resources.
 * Only connection failures reach the controller's stream outage handler.
 */
function defaultDeps(onPhase: (phase: RestartPhase) => void): RestartDeps {
  return {
    sendRestart: async () => {
      try {
        await sendMessage({ content: "/restart", images: [] });
        return { kind: "accepted" };
      } catch (error) {
        return {
          kind: "failed",
          category:
            error instanceof RequestError ? error.category : "transport",
        };
      }
    },
    openStream: (handlers) => {
      // A malformed frame is not evidence that the gateway went down.
      const source = connectChatEvents(handlers.onEvent);
      setStreamConnectionErrorHandler(source, handlers.onError);
      source.addEventListener("open", () => handlers.onOpen());
      return { close: () => source.close() };
    },
    checkStatus: async () => (await fetchGatewayStatusRaw()) !== null,
    scheduleStatusPolls: (tick) => {
      const id = globalThis.setInterval(tick, STATUS_POLL_MS);
      return () => globalThis.clearInterval(id);
    },
    onPhase,
  };
}

/**
 * Confirms and runs the gateway restart controller, then disposes it on
 * replacement or component cleanup. `makeDeps` permits injecting its
 * dependencies.
 */
export const RestartControl: Component<RestartControlProps> = (props) => {
  const { t } = useI18n();
  const [dialogOpen, setDialogOpen] = createSignal(false);
  const [phase, setPhase] = createSignal<RestartPhase>("idle");
  let controller: RestartController | undefined;

  const confirmRestart = () => {
    setDialogOpen(false);
    controller?.dispose();
    const build = props.makeDeps ?? defaultDeps;
    const deps = build(setPhase);
    controller = createRestartController({
      ...deps,
      onFailure: (category) => {
        console.error("[gateway-restart]", {
          event: "restart_failed",
          category,
        });
        deps.onFailure?.(category);
      },
    });
    controller.start();
  };

  onCleanup(() => controller?.dispose());

  return (
    <Show when={props.restartEnabled()}>
      <div class="shell-restart">
        <button
          class="shell-restart__button"
          disabled={phase() === "restarting"}
          onClick={() => setDialogOpen(true)}
          type="button"
        >
          <span
            aria-hidden="true"
            class={
              phase() === "restarting"
                ? "shell-restart__icon shell-restart__icon--spinning"
                : "shell-restart__icon"
            }
          />
          {t(phase() === "failed" ? "restart-retry" : "restart-action")}
        </button>
        <Show when={phase() === "restarting"}>
          <span class="shell-restart__status" role="status">
            {t("restart-progress")}
          </span>
        </Show>
        <Show when={phase() === "restarted"}>
          <span
            class="shell-restart__status shell-restart__status--done"
            role="status"
          >
            {t("restart-complete")}
          </span>
        </Show>

        <Show when={phase() === "failed"}>
          <span class="shell-restart__status" role="alert">
            {t("restart-failed")}
          </span>
        </Show>

        <AlertDialog onOpenChange={setDialogOpen} open={dialogOpen()}>
          <AlertDialog.Portal>
            <AlertDialog.Overlay class="dialog-overlay" />
            <AlertDialog.Content class="dialog-surface shell-restart-dialog">
              <AlertDialog.Title class="dialog-title">
                {t("restart-confirm-title")}
              </AlertDialog.Title>
              <AlertDialog.Description class="dialog-description">
                {t("restart-confirm-description")}
              </AlertDialog.Description>
              <div class="dashboard-detail__actions">
                <button
                  class="dashboard-detail__ghost"
                  onClick={confirmRestart}
                  type="button"
                >
                  {t("restart-confirm-accept")}
                </button>
                <AlertDialog.CloseButton class="dashboard-detail__ghost dashboard-detail__ghost--danger">
                  {t("restart-confirm-cancel")}
                </AlertDialog.CloseButton>
              </div>
            </AlertDialog.Content>
          </AlertDialog.Portal>
        </AlertDialog>
      </div>
    </Show>
  );
};
