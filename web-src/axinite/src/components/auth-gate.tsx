/**
 * Boot-time bearer-token gate wrapped around the router. It probes the
 * gateway once and either renders its children or prompts for a token, which
 * is kept in session storage via `lib/auth/token`.
 */

import { createSignal, type JSX, Match, onMount, Show, Switch } from "solid-js";

import { withRequestTimeout } from "@/lib/api/client";
import {
  clearGatewayToken,
  getGatewayToken,
  setGatewayToken,
} from "@/lib/auth/token";
import { useI18n } from "@/lib/i18n/provider";

type GateState = "checking" | "ready" | "locked";
type GateError = "rejected" | "unreachable" | null;
type ProbeResult = "ok" | "unauthorized" | "unreachable";

// The gateway protects /api/* with a bearer token (src/channels/web/auth.rs)
// but the mock backend accepts anonymous requests. Probe once at boot: if the
// gateway answers anonymously the gate stays open, otherwise ask for a token.
// A probe that exceeds the shared request timeout counts as unreachable.
async function probeGateway(token: string | null): Promise<ProbeResult> {
  try {
    const response = await withRequestTimeout((signal) =>
      fetch("/api/gateway/status", {
        signal,
        headers: {
          Accept: "application/json",
          ...(token ? { Authorization: `Bearer ${token}` } : {}),
        },
      })
    );
    if (response.ok) {
      return "ok";
    }
    if (response.status === 401 || response.status === 403) {
      return "unauthorized";
    }
    return "unreachable";
  } catch {
    return "unreachable";
  }
}

type AuthGateProps = {
  children: JSX.Element;
};

export const AuthGate = (props: AuthGateProps) => {
  const { t } = useI18n();
  const [state, setState] = createSignal<GateState>("checking");
  const [error, setError] = createSignal<GateError>(null);
  const [tokenInput, setTokenInput] = createSignal("");
  const [submitting, setSubmitting] = createSignal(false);

  const applyProbe = (result: ProbeResult, hadToken: boolean) => {
    if (result === "ok") {
      setError(null);
      setState("ready");
      return;
    }
    if (result === "unauthorized") {
      clearGatewayToken();
      setError(hadToken ? "rejected" : null);
      setState("locked");
      return;
    }
    setError("unreachable");
    setState("locked");
  };

  onMount(async () => {
    // The Python e2e suite (and shared deep links) hand off the bearer token
    // as a `?token=` query parameter. Consume it before probing: store it,
    // then strip it from the URL so it does not linger in history, referrers,
    // or copied links. The path and any other query parameters are preserved.
    const url = new URL(window.location.href);
    const queryToken = url.searchParams.get("token");
    if (queryToken) {
      setGatewayToken(queryToken);
      url.searchParams.delete("token");
      const stripped = `${url.pathname}${url.search}${url.hash}`;
      window.history.replaceState(window.history.state, "", stripped);
    }

    // The boot probe always passes `hadToken = false`, whether or not a token
    // was stored: a stored token that no longer works is cleared and the form
    // shown afresh, without a rejection notice from a previous session.
    applyProbe(await probeGateway(getGatewayToken()), false);
  });

  const submit = async (event: Event) => {
    event.preventDefault();
    const candidate = tokenInput().trim();
    if (candidate.length === 0 || submitting()) {
      return;
    }
    setSubmitting(true);
    try {
      const result = await probeGateway(candidate);
      if (result === "ok") {
        setGatewayToken(candidate);
      }
      applyProbe(result, true);
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Switch>
      <Match when={state() === "checking"}>
        <main class="auth-gate" id="auth-screen" aria-busy="true">
          <p class="auth-gate__status">{t("auth-checking")}</p>
        </main>
      </Match>
      <Match when={state() === "locked"}>
        <main class="auth-gate" id="auth-screen">
          <form class="auth-gate__panel catalogue-form" onSubmit={submit}>
            <h1 class="auth-gate__title">{t("auth-title")}</h1>
            <p class="auth-gate__description">{t("auth-description")}</p>
            <label class="catalogue-form__label" for="auth-gate-token">
              {t("auth-token-label")}
            </label>
            <div class="catalogue-form__row">
              <input
                id="auth-gate-token"
                class="catalogue-form__input"
                type="password"
                autocomplete="off"
                value={tokenInput()}
                onInput={(event) => setTokenInput(event.currentTarget.value)}
              />
              <button
                type="submit"
                class="btn btn-primary btn-sm"
                disabled={submitting()}
              >
                {t("auth-submit")}
              </button>
            </div>
            <Show when={error() === "rejected"}>
              <p class="auth-gate__error" role="alert">
                {t("auth-error-rejected")}
              </p>
            </Show>
            <Show when={error() === "unreachable"}>
              <p class="auth-gate__error" role="alert">
                {t("auth-error-unreachable")}
              </p>
            </Show>
          </form>
        </main>
      </Match>
      <Match when={state() === "ready"}>{props.children}</Match>
    </Switch>
  );
};
