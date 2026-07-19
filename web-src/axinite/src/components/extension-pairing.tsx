import { createMutation, createQuery } from "@tanstack/solid-query";
import type { Component } from "solid-js";
import { createSignal, For, Show } from "solid-js";

import { approvePairing, fetchPairingRequests } from "@/lib/api/pairing";
import { useI18n } from "@/lib/i18n/provider";

const PAIRING_POLL_MS = 10_000;

type ExtensionPairingProps = {
  channel: string;
  /** Refresh installed extensions after a successful approval. */
  onApproved: () => void;
};

export const ExtensionPairing: Component<ExtensionPairingProps> = (props) => {
  const { t } = useI18n();
  const [errors, setErrors] = createSignal<Record<string, string>>({});

  const pairing = createQuery(() => ({
    queryKey: ["pairing", props.channel],
    queryFn: () => fetchPairingRequests(props.channel),
    refetchInterval: PAIRING_POLL_MS,
  }));

  const approveMutation = createMutation(() => ({
    mutationFn: (code: string) => approvePairing(props.channel, code),
    onSuccess: (_result, code) => {
      setErrors((current) => {
        const next = { ...current };
        delete next[code];
        return next;
      });
      props.onApproved();
      void pairing.refetch();
    },
    onError: (error: unknown, code) => {
      setErrors((current) => ({
        ...current,
        [code]: error instanceof Error ? error.message : String(error),
      }));
    },
  }));

  const requests = () => pairing.data?.requests ?? [];

  return (
    <Show when={requests().length > 0}>
      <div class="ext-pairing" data-channel={props.channel}>
        <div class="pairing-heading">{t("extensions-pairing-heading")}</div>
        <For each={requests()}>
          {(request) => (
            <div class="pairing-row">
              <span class="pairing-code">{request.code}</span>
              <span class="pairing-sender">
                {t("extensions-pairing-from", { sender: request.sender_id })}
              </span>
              <button
                aria-label={t("extensions-pairing-approve-label", {
                  code: request.code,
                })}
                class="catalogue-card__action"
                disabled={approveMutation.isPending}
                onClick={() => approveMutation.mutate(request.code)}
                type="button"
              >
                {t("extensions-pairing-approve")}
              </button>
              <Show when={errors()[request.code]}>
                <p class="pairing-row__error" role="alert">
                  {errors()[request.code]}
                </p>
              </Show>
            </div>
          )}
        </For>
      </div>
    </Show>
  );
};
