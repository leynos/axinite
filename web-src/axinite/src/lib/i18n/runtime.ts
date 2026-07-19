import i18n from "i18next";
import LanguageDetector from "i18next-browser-languagedetector";
import Fluent from "i18next-fluent";
import FluentBackend from "i18next-fluent-backend";

import { normaliseBasePath } from "@/lib/base-path";
import {
  AVAILABLE_LOCALES,
  DEFAULT_LOCALE,
  DETECTION_ORDER,
  getLocaleDirection,
  getLocaleMetadata,
} from "@/lib/i18n/supported-locales";

const supportedLanguages = AVAILABLE_LOCALES.map((locale) => locale.code);

type AjaxResponse = {
  status: number;
  statusText?: string;
};

type AjaxOptions = {
  body?: BodyInit | null;
  headers?: Record<string, string>;
  method?: string;
  withCredentials?: boolean;
};

function fetchAjax(
  url: string,
  options: AjaxOptions = {},
  callback: (data: string | Error, xhr: AjaxResponse) => void
): void {
  const request: RequestInit = {
    credentials: options.withCredentials ? "include" : "same-origin",
    method: options.method ?? "GET",
    headers: options.headers,
  };

  if (options.body != null) {
    request.body = options.body;
  }

  void fetch(url, request)
    .then(async (response) => {
      if (!response.ok) {
        const statusText =
          response.statusText ||
          `Request failed with status ${response.status}`;
        callback(new Error(statusText), {
          status: response.status,
          statusText,
        });
        return;
      }

      callback(await response.text(), {
        status: response.status,
        statusText: response.statusText,
      });
    })
    .catch((error) => {
      const typedError =
        error instanceof Error
          ? error
          : new Error("Unexpected i18n fetch failure");
      callback(typedError, {
        status: 500,
        statusText: typedError.message,
      });
    });
}

export function buildFluentLoadPath(rawBase: string | undefined): string {
  return `${normaliseBasePath(rawBase)}locales/{{lng}}/{{ns}}.ftl`;
}

export function applyDocumentLocale(language: string | undefined): void {
  if (typeof document === "undefined") {
    return;
  }

  const locale = getLocaleMetadata(language ?? DEFAULT_LOCALE);
  const direction = getLocaleDirection(locale.code);
  document.documentElement.lang = locale.code;
  document.documentElement.dir = direction;
  document.documentElement.dataset.direction = direction;
  if (document.body) {
    document.body.dataset.direction = direction;
  }
}

const i18nReady = i18n
  .use(FluentBackend)
  .use(LanguageDetector)
  .use(Fluent)
  .init({
    backend: {
      loadPath: buildFluentLoadPath(
        import.meta.env.BASE_URL as string | undefined
      ),
      ajax: fetchAjax,
    },
    fallbackLng: DEFAULT_LOCALE,
    supportedLngs: supportedLanguages,
    ns: ["common"],
    defaultNS: "common",
    interpolation: {
      escapeValue: false,
    },
    detection: {
      order: [...DETECTION_ORDER],
      lookupQuerystring: "lng",
      caches: ["localStorage"],
    },
    returnNull: false,
    i18nFormat: {
      fluentBundleOptions: {
        // Disable U+2068/U+2069 isolating marks so Fluent does not inject bidi
        // control characters that interfere with host renderers and validation.
        useIsolating: false,
      },
    },
  });

void i18nReady
  .then(() => {
    applyDocumentLocale(
      i18n.resolvedLanguage ?? i18n.language ?? DEFAULT_LOCALE
    );
  })
  .catch((error) => {
    console.error("[i18n] Failed to initialize runtime locale", error);
  });

i18n.on("languageChanged", (nextLanguage) => {
  applyDocumentLocale(nextLanguage);
});

export { i18nReady };
export default i18n;
