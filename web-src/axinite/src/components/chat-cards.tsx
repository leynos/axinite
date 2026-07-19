import { createSignal, Show } from "solid-js";

import { buildAppPath } from "@/lib/base-path";
import { useI18n } from "@/lib/i18n/provider";

/**
 * Returns true only for absolute http(s) URLs. Everything else — relative
 * references, `javascript:` payloads, `data:` URIs, `file:` paths, and
 * unparseable strings — is rejected. This is the single guard that gates any
 * externally supplied URL before it reaches `window.open` or an anchor.
 */
export function isHttpUrl(value: string): boolean {
  let parsed: URL;
  try {
    parsed = new URL(value);
  } catch {
    return false;
  }
  return parsed.protocol === "http:" || parsed.protocol === "https:";
}

/**
 * Opens an external URL in a new tab, but only when it is a genuine http(s)
 * URL. Returns whether the URL was considered safe and opened.
 */
export function openExternalUrl(value: string): boolean {
  if (!isHttpUrl(value)) {
    return false;
  }
  window.open(value, "_blank", "noopener,noreferrer");
  return true;
}

export const GeneratedImageCard = (props: {
  dataUrl: string;
  path?: string;
}) => {
  const { t } = useI18n();
  return (
    <div class="chat-preview__turn chat-preview__turn--assistant">
      <div class="chat-preview__bubble chat-preview__bubble--assistant">
        <figure class="chat-preview__generated-image">
          <img
            alt={t("chat-generated-image-alt")}
            class="chat-preview__generated-image-media"
            src={props.dataUrl}
          />
          <Show when={props.path}>
            <figcaption class="chat-preview__generated-image-caption">
              {props.path}
            </figcaption>
          </Show>
        </figure>
      </div>
    </div>
  );
};

export const JobStartCard = (props: {
  jobId: string;
  title: string;
  browseUrl?: string;
}) => {
  const { t } = useI18n();
  const basePath = import.meta.env.BASE_URL as string | undefined;
  const shortId = () => props.jobId.slice(0, 8);
  const displayTitle = () =>
    props.title.trim().length > 0
      ? props.title
      : t("chat-job-card-fallback-title");

  return (
    <div class="chat-preview__turn chat-preview__turn--assistant">
      <div class="chat-preview__bubble chat-preview__bubble--assistant">
        <div class="chat-preview__job-card">
          <div class="chat-preview__job-card-heading">
            <span class="chat-preview__job-card-title">{displayTitle()}</span>
            <span class="chat-preview__job-card-id">
              {t("chat-job-card-id", { id: shortId() })}
            </span>
          </div>
          <div class="chat-preview__job-card-actions">
            <a
              class="chat-preview__job-card-link"
              href={buildAppPath(basePath, "jobs")}
            >
              {t("chat-job-card-open")}
            </a>
            <Show when={props.browseUrl && isHttpUrl(props.browseUrl)}>
              <a
                class="chat-preview__job-card-link"
                href={props.browseUrl}
                rel="noopener noreferrer"
                target="_blank"
              >
                {t("chat-job-card-browse")}
              </a>
            </Show>
          </div>
        </div>
      </div>
    </div>
  );
};

export const AuthCard = (props: {
  extensionName: string;
  instructions?: string;
  authUrl?: string;
  setupUrl?: string;
  onSubmit: (token: string) => Promise<boolean>;
  onCancel: () => void;
}) => {
  const { t } = useI18n();
  const [token, setToken] = createSignal("");
  const [pending, setPending] = createSignal(false);
  const [hasError, setHasError] = createSignal(false);

  async function handleSubmit(): Promise<void> {
    if (token().length === 0 || pending()) {
      return;
    }
    setPending(true);
    setHasError(false);
    try {
      const ok = await props.onSubmit(token());
      // On success the parent removes this card, so no local reset is needed.
      if (!ok) {
        setHasError(true);
      }
    } catch {
      setHasError(true);
    } finally {
      setPending(false);
    }
  }

  return (
    <div class="chat-preview__turn chat-preview__turn--assistant">
      <div class="chat-preview__bubble chat-preview__bubble--assistant">
        <div class="chat-preview__auth-card">
          <h3 class="chat-preview__auth-card-title">
            {t("chat-auth-card-title", { name: props.extensionName })}
          </h3>
          <Show when={props.instructions}>
            <p class="chat-preview__auth-card-instructions">
              {props.instructions}
            </p>
          </Show>

          {/*
            When the daemon supplies an `auth_url` we render an OAuth button.
            When it is absent, the legacy UI opened the extension configure
            modal. The Solid chat surface must not reach into the extensions
            screen, so we render this same card in token-only mode (no OAuth
            button) and rely on the manual token path instead.
          */}
          <Show when={props.authUrl && isHttpUrl(props.authUrl)}>
            <button
              class="chat-preview__auth-card-oauth"
              type="button"
              onClick={() => {
                if (props.authUrl) {
                  openExternalUrl(props.authUrl);
                }
              }}
            >
              {t("chat-auth-card-oauth")}
            </button>
          </Show>

          <Show when={props.setupUrl && isHttpUrl(props.setupUrl)}>
            <a
              class="chat-preview__auth-card-setup"
              href={props.setupUrl}
              rel="noopener noreferrer"
              target="_blank"
            >
              {t("chat-auth-card-get-token")}
            </a>
          </Show>

          <label class="chat-preview__auth-card-field">
            <span>{t("chat-auth-card-token-label")}</span>
            <input
              class="chat-preview__auth-card-input"
              disabled={pending()}
              onInput={(event) => setToken(event.currentTarget.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  void handleSubmit();
                }
              }}
              placeholder={t("chat-auth-card-token-placeholder")}
              type="password"
              value={token()}
            />
          </label>

          <Show when={hasError()}>
            <p class="chat-preview__auth-card-error" role="alert">
              {t("chat-auth-card-error")}
            </p>
          </Show>

          <div class="chat-preview__auth-card-actions">
            <button
              class="chat-preview__auth-card-submit"
              disabled={token().length === 0 || pending()}
              type="button"
              onClick={() => void handleSubmit()}
            >
              {t("chat-auth-card-submit")}
            </button>
            <button
              class="chat-preview__auth-card-cancel"
              disabled={pending()}
              type="button"
              onClick={() => props.onCancel()}
            >
              {t("chat-auth-card-cancel")}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};
