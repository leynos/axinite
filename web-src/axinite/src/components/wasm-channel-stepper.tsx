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

/**
 * Compute the three-step activation stepper model for a WASM channel, mirroring
 * the legacy `renderWasmChannelStepper` heuristic.
 */
export function computeStepperModel(activationStatus?: string): StepperModel {
  const status = activationStatus || "installed";

  let reachedIdx: number;
  if (status === "active" || status === "pairing" || status === "failed") {
    reachedIdx = 2;
  } else if (status === "configured") {
    reachedIdx = 1;
  } else {
    reachedIdx = 0;
  }

  const states: StepState[] = [];
  for (let i = 0; i < STEP_COUNT; i += 1) {
    if (i < reachedIdx) {
      states.push("completed");
      continue;
    }
    if (i === reachedIdx) {
      if (status === "failed") {
        states.push("failed");
      } else if (status === "pairing") {
        states.push("in-progress");
      } else if (
        status === "active" ||
        status === "configured" ||
        status === "installed"
      ) {
        states.push("completed");
      } else {
        states.push("pending");
      }
      continue;
    }
    states.push("pending");
  }

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
