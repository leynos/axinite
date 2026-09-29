/**
 * Language selector shown in the app shell. It lists the locales that
 * `supported-locales` marks as complete and switches language through the
 * i18n provider.
 */

import { For } from "solid-js";

import { useI18n } from "@/lib/i18n/provider";

export const LocalePicker = () => {
  const { changeLanguage, language, locales, t } = useI18n();

  return (
    <label class="shell-locale">
      <span class="shell-locale__label">{t("locale-picker-label")}</span>
      <select
        class="select select-bordered select-sm shell-locale__select"
        value={language()}
        onInput={(event) => void changeLanguage(event.currentTarget.value)}
      >
        <For each={locales}>
          {(locale) => (
            <option value={locale.code}>
              {locale.nativeLabel} · {locale.label}
            </option>
          )}
        </For>
      </select>
    </label>
  );
};
