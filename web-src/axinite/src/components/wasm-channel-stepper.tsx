import type { Component } from "solid-js";
import { For, Show } from "solid-js";

import { useI18n } from "@/lib/i18n/provider";

export type StepState = "completed" | "failed" | "in-progress" | "pending";

export type StepperModel = {
  reachedIdx: number;
  states: StepState[];
  awaitingPairing: boolean;
};

const STEP_COUNT = 3;

// Per-status derivation of the stepper model: the furthest step index reached
// and the state rendered at that index. Steps before the reached index are
// always "completed" and steps after it are always "pending". Statuses absent
// from the table fall back to DEFAULT_STATUS_ENTRY.
type StatusEntry = { reachedIdx: number; reachedState: StepState };

const STATUS_TABLE: Record<string, StatusEntry> = {
  active: { reachedIdx: 2, reachedState: "completed" },
  pairing: { reachedIdx: 2, reachedState: "in-progress" },
  failed: { reachedIdx: 2, reachedState: "failed" },
  configured: { reachedIdx: 1, reachedState: "completed" },
  installed: { reachedIdx: 0, reachedState: "completed" },
};

const DEFAULT_STATUS_ENTRY: StatusEntry = {
  reachedIdx: 0,
  reachedState: "pending",
};

/**
 * Compute the three-step activation stepper model for a WASM channel, mirroring
 * the legacy `renderWasmChannelStepper` heuristic.
 */
export function computeStepperModel(activationStatus?: string): StepperModel {
  const status = activationStatus || "installed";
  const { reachedIdx, reachedState } =
    STATUS_TABLE[status] ?? DEFAULT_STATUS_ENTRY;

  const states: StepState[] = Array.from({ length: STEP_COUNT }, (_, i) => {
    if (i < reachedIdx) {
      return "completed";
    }
    return i === reachedIdx ? reachedState : "pending";
  });

  return { reachedIdx, states, awaitingPairing: status === "pairing" };
}

export const WasmChannelStepper: Component<{ activationStatus?: string }> = (
  props
) => {
  const { t } = useI18n();
  const model = () => computeStepperModel(props.activationStatus);
  const stepLabel = (index: number) => {
    if (index === 0) {
      return t("extensions-stepper-installed");
    }
    if (index === 1) {
      return t("extensions-stepper-configured");
    }
    return model().awaitingPairing
      ? t("extensions-stepper-awaiting-pairing")
      : t("extensions-stepper-active");
  };
  const stateLabel = (state: StepState) => {
    switch (state) {
      case "completed":
        return t("extensions-stepper-state-completed");
      case "failed":
        return t("extensions-stepper-state-failed");
      case "in-progress":
        return t("extensions-stepper-state-in-progress");
      default:
        return t("extensions-stepper-state-pending");
    }
  };

  return (
    <ol aria-label={t("extensions-stepper-label")} class="ext-stepper">
      <For each={model().states}>
        {(state, index) => (
          <li class={`stepper-step stepper-step--${state}`}>
            <span aria-hidden="true" class="stepper-circle">
              <Show when={state === "completed"}>✓</Show>
              <Show when={state === "failed"}>✗</Show>
            </span>
            <span class="stepper-label">{stepLabel(index())}</span>
            <span class="stepper-state-label">{stateLabel(state)}</span>
          </li>
        )}
      </For>
    </ol>
  );
};
