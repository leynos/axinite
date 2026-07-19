import { AlertDialog } from "@kobalte/core/alert-dialog";
import type { Component } from "solid-js";
import { createSignal, onCleanup, Show } from "solid-js";

import { connectChatEvents, sendMessage } from "@/lib/api/chat";
import { fetchGatewayStatusRaw } from "@/lib/api/gateway";
import { useI18n } from "@/lib/i18n/provider";
import {
  createRestartController,
  type RestartController,
  type RestartDeps,
  type RestartPhase,
} from "@/lib/restart";

const STATUS_POLL_MS = 3_000;

type RestartControlProps = {
  restartEnabled: () => boolean | undefined;
  /** Injectable controller dependencies; defaults wire the live gateway. */
  makeDeps?: (onPhase: (phase: RestartPhase) => void) => RestartDeps;
};

function defaultDeps(onPhase: (phase: RestartPhase) => void): RestartDeps {
  return {
    sendRestart: () => sendMessage({ content: "/restart", images: [] }),
    openStream: (handlers) => {
      const source = connectChatEvents(handlers.onEvent, handlers.onError);
      source.onopen = () => handlers.onOpen();
      return { close: () => source.close() };
    },
    checkStatus: async () => (await fetchGatewayStatusRaw()) !== null,
    scheduleStatusPolls: (tick) => {
      const id = window.setInterval(tick, STATUS_POLL_MS);
      return () => window.clearInterval(id);
    },
    onPhase,
  };
}

export const RestartControl: Component<RestartControlProps> = (props) => {
  const { t } = useI18n();
  const [dialogOpen, setDialogOpen] = createSignal(false);
  const [phase, setPhase] = createSignal<RestartPhase>("idle");
  let controller: RestartController | undefined;

  const confirmRestart = () => {
    setDialogOpen(false);
    controller?.dispose();
    const build = props.makeDeps ?? defaultDeps;
    controller = createRestartController(build(setPhase));
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
          {t("restart-action")}
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
