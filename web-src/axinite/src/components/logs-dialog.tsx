import { Dialog } from "@kobalte/core/dialog";
import { createMutation, createQuery } from "@tanstack/solid-query";
import { createEffect, createSignal, For, onCleanup } from "solid-js";
import type { LogEntry } from "@/lib/api/contracts";
import { connectLogEvents, fetchLogLevel, setLogLevel } from "@/lib/api/logs";
import { useI18n } from "@/lib/i18n/provider";

function formatTimestamp(value: string): string {
  return new Intl.DateTimeFormat("en-GB", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  }).format(new Date(value));
}

export const LogsDialog = () => {
  const { t } = useI18n();
  const [entries, setEntries] = createSignal<LogEntry[]>([]);

  const level = createQuery(() => ({
    queryKey: ["logs", "level"],
    queryFn: fetchLogLevel,
  }));

  const levelMutation = createMutation(() => ({
    mutationFn: (nextLevel: string) => setLogLevel(nextLevel),
  }));

  createEffect(() => {
    const source = connectLogEvents((entry) => {
      setEntries((current) => [entry, ...current].slice(0, 30));
    });
    onCleanup(() => source.close());
  });

  return (
    <Dialog>
      <Dialog.Trigger class="btn btn-ghost btn-sm shell-button">
        {t("logs-button")}
      </Dialog.Trigger>
      <Dialog.Portal>
        <Dialog.Overlay class="dialog-overlay" />
        <Dialog.Content class="dialog-surface">
          <Dialog.Title class="dialog-title">{t("logs-title")}</Dialog.Title>
          <Dialog.Description class="dialog-description">
            {t("logs-description")}
          </Dialog.Description>

          <div class="catalogue-form">
            <label class="catalogue-form__label" for="logs-level">
              {t("logs-level-label")}
            </label>
            <div class="catalogue-form__row">
              <select
                class="catalogue-form__input"
                id="logs-level"
                onChange={(event) =>
                  levelMutation.mutate(event.currentTarget.value)
                }
                value={level.data?.level ?? "info"}
              >
                <option value="debug">{t("logs-level-debug")}</option>
                <option value="info">{t("logs-level-info")}</option>
                <option value="warn">{t("logs-level-warn")}</option>
                <option value="error">{t("logs-level-error")}</option>
              </select>
            </div>
          </div>

          <div class="logs-panel" aria-live="polite">
            <For each={entries()}>
              {(entry) => (
                <article class="logs-panel__item">
                  <p class="logs-panel__time">
                    {formatTimestamp(entry.timestamp)} · {entry.level}
                  </p>
                  <p class="logs-panel__message">
                    [{entry.source}] {entry.message}
                  </p>
                </article>
              )}
            </For>
          </div>
          <div class="dialog-actions">
            <Dialog.CloseButton class="btn btn-primary btn-sm">
              {t("dialog-close")}
            </Dialog.CloseButton>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog>
  );
};
