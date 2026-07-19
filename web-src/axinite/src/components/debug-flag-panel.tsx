import { Collapsible } from "@kobalte/core/collapsible";
import { For } from "solid-js";

import { FEATURE_FLAGS } from "@/lib/feature-flags/registry";
import { useFeatureFlags } from "@/lib/feature-flags/runtime";
import { useI18n } from "@/lib/i18n/provider";

export const DebugFlagPanel = () => {
  const flags = useFeatureFlags();
  const { t } = useI18n();

  return (
    <Collapsible class="debug-panel" defaultOpen={false}>
      <div class="debug-panel__header">
        <div>
          <p class="debug-panel__eyebrow">{t("debug-eyebrow")}</p>
          <h2 class="debug-panel__title">{t("debug-title")}</h2>
        </div>
        <Collapsible.Trigger class="btn btn-outline btn-sm shell-button">
          {t("debug-toggle")}
        </Collapsible.Trigger>
      </div>
      <Collapsible.Content class="debug-panel__content">
        <p class="debug-panel__description">{t("debug-description")}</p>
        <table aria-label={t("debug-table-label")} class="debug-table">
          <thead class="debug-table__header">
            <tr>
              <th scope="col">{t("debug-column-flag")}</th>
              <th scope="col">{t("debug-column-state")}</th>
              <th scope="col">{t("debug-column-actions")}</th>
            </tr>
          </thead>
          <tbody>
            <For each={FEATURE_FLAGS}>
              {(flag) => {
                const resolvedFlag = () => flags.resolvedFlags()[flag.name];
                const override = () => flags.overrides()[flag.name];

                return (
                  <tr class="debug-table__row">
                    <th class="debug-table__cell" scope="row">
                      <p class="debug-table__name">{flag.name}</p>
                      <p class="debug-table__meta">
                        {flag.owner} · {flag.backendContract}
                      </p>
                    </th>
                    <td class="debug-table__cell">
                      <span class="badge badge-outline">
                        {t(`flag-${resolvedFlag() ? "on" : "off"}`)}
                      </span>
                    </td>
                    <td class="debug-table__cell debug-table__actions">
                      <button
                        class="btn btn-outline btn-xs shell-button"
                        type="button"
                        onClick={() => flags.setOverride(flag.name, true)}
                      >
                        {t("flag-force-on")}
                      </button>
                      <button
                        class="btn btn-outline btn-xs shell-button"
                        type="button"
                        onClick={() => flags.setOverride(flag.name, false)}
                      >
                        {t("flag-force-off")}
                      </button>
                      <button
                        class="btn btn-ghost btn-xs shell-button"
                        type="button"
                        disabled={typeof override() === "undefined"}
                        onClick={() => flags.clearOverride(flag.name)}
                      >
                        {t("flag-clear-override")}
                      </button>
                    </td>
                  </tr>
                );
              }}
            </For>
          </tbody>
        </table>
      </Collapsible.Content>
    </Collapsible>
  );
};
