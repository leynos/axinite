import type { Accessor, ParentComponent } from "solid-js";
import { createContext, createSignal, onCleanup, useContext } from "solid-js";

import i18n, { i18nReady } from "@/lib/i18n/runtime";
import {
  AVAILABLE_LOCALES,
  DEFAULT_LOCALE,
  type SupportedLocale,
} from "@/lib/i18n/supported-locales";

type TranslationOptions = Record<string, string | number>;

type I18nContextValue = {
  language: Accessor<string>;
  locales: readonly SupportedLocale[];
  t: (key: string, options?: TranslationOptions) => string;
  changeLanguage: (nextLanguage: string) => Promise<void>;
};

const I18nContext = createContext<I18nContextValue>();

export const I18nProvider: ParentComponent = (props) => {
  const [language, setLanguage] = createSignal<string>(DEFAULT_LOCALE);

  const handleLanguageChanged = (nextLanguage: string) => {
    setLanguage(nextLanguage);
  };

  void i18nReady
    .then(() => {
      setLanguage(i18n.resolvedLanguage ?? i18n.language ?? DEFAULT_LOCALE);
    })
    .catch((error) => {
      console.error(
        "[i18n] Failed to initialize provider language state",
        error
      );
    });

  i18n.on("languageChanged", handleLanguageChanged);
  onCleanup(() => {
    i18n.off("languageChanged", handleLanguageChanged);
  });

  const t = (key: string, options?: TranslationOptions) => {
    language();
    return i18n.t(key, options) as string;
  };

  return (
    <I18nContext.Provider
      value={{
        language,
        locales: AVAILABLE_LOCALES,
        t,
        changeLanguage: async (nextLanguage) => {
          await i18n.changeLanguage(nextLanguage);
        },
      }}
    >
      {props.children}
    </I18nContext.Provider>
  );
};

export function useI18n(): I18nContextValue {
  const context = useContext(I18nContext);

  if (!context) {
    throw new Error("I18nProvider is missing");
  }

  return context;
}
