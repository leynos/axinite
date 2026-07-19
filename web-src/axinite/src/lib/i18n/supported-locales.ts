export type TextDirection = "ltr" | "rtl";

export type SupportedLocale = {
  code: string;
  label: string;
  nativeLabel: string;
  direction?: TextDirection;
};

export const SUPPORTED_LOCALES = [
  { code: "en-GB", label: "English (UK)", nativeLabel: "English (UK)" },
  { code: "fr", label: "French", nativeLabel: "Français" },
  { code: "de", label: "German", nativeLabel: "Deutsch" },
  { code: "it", label: "Italian", nativeLabel: "Italiano" },
  { code: "nl", label: "Dutch", nativeLabel: "Nederlands" },
  { code: "pl", label: "Polish", nativeLabel: "Polski" },
  { code: "hi", label: "Hindi", nativeLabel: "हिन्दी" },
  { code: "ja", label: "Japanese", nativeLabel: "日本語" },
  { code: "zh-CN", label: "Chinese (Simplified)", nativeLabel: "简体中文" },
  { code: "ar", label: "Arabic", nativeLabel: "العربية", direction: "rtl" },
] as const satisfies Readonly<[SupportedLocale, ...SupportedLocale[]]>;

export const COMPLETE_LOCALE_CODES = [
  "en-GB",
  "fr",
  "de",
  "it",
  "nl",
  "pl",
  "hi",
  "ja",
  "zh-CN",
  "ar",
] as const satisfies readonly [string, ...string[]];

const COMPLETE_LOCALE_CODE_SET = new Set<string>(COMPLETE_LOCALE_CODES);

export const AVAILABLE_LOCALES = SUPPORTED_LOCALES.filter((locale) =>
  COMPLETE_LOCALE_CODE_SET.has(locale.code)
);

export const DEFAULT_LOCALE = SUPPORTED_LOCALES[0].code;
export const DETECTION_ORDER = [
  "querystring",
  "localStorage",
  "navigator",
] as const;

const LOCALE_MAP: Record<string, SupportedLocale> = SUPPORTED_LOCALES.reduce(
  (map, locale) => {
    const fullCode = locale.code.toLowerCase();
    map[fullCode] = locale;
    const [languagePart] = fullCode.split("-");
    if (languagePart && !map[languagePart]) {
      map[languagePart] = locale;
    }
    return map;
  },
  {} as Record<string, SupportedLocale>
);

const fallbackLocale = (() => {
  const locale = LOCALE_MAP[DEFAULT_LOCALE.toLowerCase()];

  if (!locale) {
    throw new Error(
      `DEFAULT_LOCALE '${DEFAULT_LOCALE}' is not present in SUPPORTED_LOCALES`
    );
  }

  return locale;
})();

export function getLocaleMetadata(code: string | undefined): SupportedLocale {
  if (!code) {
    return fallbackLocale;
  }

  const lookup = code.toLowerCase();
  const [languagePart] = lookup.split("-");
  return (
    LOCALE_MAP[lookup] ??
    (languagePart ? LOCALE_MAP[languagePart] : undefined) ??
    fallbackLocale
  );
}

export function getLocaleDirection(code: string | undefined): TextDirection {
  return getLocaleMetadata(code).direction ?? "ltr";
}
