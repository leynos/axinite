import {
  createMutation,
  createQuery,
  keepPreviousData,
  useQueryClient,
} from "@tanstack/solid-query";
import type { Accessor } from "solid-js";
import { createEffect, createMemo, createSignal, For, Show } from "solid-js";

import type {
  RoutineDetailResponse,
  RoutineInfo,
  RoutineRunInfo,
} from "@/lib/api/contracts";
import {
  deleteRoutine,
  fetchRoutineDetail,
  fetchRoutineRuns,
  fetchRoutineSummary,
  fetchRoutines,
  toggleRoutine,
  triggerRoutine,
} from "@/lib/api/routines";
import { useI18n } from "@/lib/i18n/provider";
import { capitalize, pascalCase } from "@/lib/string-case";

const STATUS_CLASS: Record<string, string> = {
  active: "pill pill--success",
  disabled: "pill pill--neutral",
  failing: "pill pill--danger",
};

const TRIGGER_CLASS: Record<string, string> = {
  cron: "pill pill--info",
  event: "pill pill--violet",
  system_event: "pill pill--warning",
  manual: "pill pill--neutral",
};

const ACTION_CLASS: Record<string, string> = {
  full_job: "pill pill--violet",
  lightweight: "pill pill--neutral",
};

function toKebabSegment(value: string): string {
  return pascalCase(value)
    .replace(/([A-Z])/g, "-$1")
    .toLowerCase()
    .replace(/^-/, "");
}

function formatTimestamp(
  value: string | null | undefined,
  fallback: string
): string {
  if (!value) {
    return fallback;
  }
  return new Intl.DateTimeFormat("en-GB", {
    day: "2-digit",
    month: "short",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(value));
}

type SummaryCard = { key: string; value: number };

// Narrow-props subcomponent for the dashboard summary cards. The `cards`
// accessor is passed through untouched so reactivity is preserved.
const SummaryCards = (props: { cards: Accessor<SummaryCard[]> }) => {
  const { t } = useI18n();
  return (
    <div class="dashboard-summary">
      <For each={props.cards()}>
        {(card) => (
          <article class="dashboard-summary__card">
            <p class="dashboard-summary__label">
              {t(`routines-summary-${toKebabSegment(card.key)}`)}
            </p>
            <p class="dashboard-summary__value">{card.value}</p>
          </article>
        )}
      </For>
    </div>
  );
};

// Narrow-props subcomponent for the routines table. Accessors are forwarded so
// the selection highlight and row list stay reactive.
const RoutinesTable = (props: {
  routines: Accessor<RoutineInfo[]>;
  activeRoutineId: Accessor<string | undefined>;
  onSelect: (id: string) => void;
}) => {
  const { t } = useI18n();
  return (
    <section class="dashboard-panel">
      <div class="dashboard-panel__header">
        <div>
          <h3 class="dashboard-panel__title">{t("routines-table-title")}</h3>
          <p class="dashboard-panel__body">{t("page-routines-agenda")}</p>
        </div>
      </div>

      <div class="dashboard-table-wrap">
        <table class="dashboard-table dashboard-table--routines">
          <thead>
            <tr>
              <th scope="col">{t("routines-column-name")}</th>
              <th scope="col">{t("routines-column-trigger")}</th>
              <th scope="col">{t("routines-column-action")}</th>
              <th scope="col">{t("routines-column-last-run")}</th>
              <th scope="col">{t("routines-column-next-run")}</th>
              <th scope="col">{t("routines-column-runs")}</th>
              <th scope="col">{t("routines-column-status")}</th>
              <th scope="col">{t("routines-column-actions")}</th>
            </tr>
          </thead>
          <tbody>
            <For each={props.routines()}>
              {(routine) => (
                <tr
                  class={
                    props.activeRoutineId() === routine.id
                      ? "dashboard-table__row dashboard-table__row--active"
                      : "dashboard-table__row"
                  }
                >
                  <td>
                    <button
                      class="dashboard-table__title-button"
                      onClick={() => props.onSelect(routine.id)}
                      type="button"
                    >
                      {routine.name}
                    </button>
                  </td>
                  <td>
                    <span
                      class={
                        TRIGGER_CLASS[routine.trigger_type] ??
                        "pill pill--neutral"
                      }
                    >
                      {routine.trigger_type}
                    </span>
                  </td>
                  <td>
                    <span
                      class={
                        ACTION_CLASS[routine.action_type] ??
                        "pill pill--neutral"
                      }
                    >
                      {routine.action_type}
                    </span>
                  </td>
                  <td class="dashboard-table__meta">
                    {formatTimestamp(
                      routine.last_run_at,
                      t("timestamp-pending")
                    )}
                  </td>
                  <td class="dashboard-table__meta">
                    {formatTimestamp(
                      routine.next_fire_at,
                      t("timestamp-pending")
                    )}
                  </td>
                  <td class="dashboard-table__meta">{routine.run_count}</td>
                  <td>
                    <span
                      class={
                        STATUS_CLASS[routine.status] ?? "pill pill--neutral"
                      }
                    >
                      {t(
                        `routines-status-${capitalize(routine.status).toLowerCase()}`
                      )}
                    </span>
                  </td>
                  <td>
                    <button
                      class="dashboard-table__action"
                      onClick={() => props.onSelect(routine.id)}
                      type="button"
                    >
                      {t("routines-action-inspect")}
                    </button>
                  </td>
                </tr>
              )}
            </For>
          </tbody>
        </table>
      </div>
    </section>
  );
};

// Narrow-props subcomponent for the active routine detail panel. The `routine`
// and `runs` accessors are forwarded verbatim, and the mutation triggers are
// passed as plain callbacks, so behaviour and reactivity are unchanged.
const RoutineDetailPanel = (props: {
  routine: Accessor<RoutineDetailResponse>;
  runs: Accessor<RoutineRunInfo[]>;
  onTrigger: () => void;
  onToggle: () => void;
  onDelete: () => void;
}) => {
  const { t } = useI18n();
  return (
    <section class="dashboard-detail">
      <div class="dashboard-detail__header">
        <div>
          <p class="dashboard-detail__eyebrow">
            {t("routines-detail-eyebrow")}
          </p>
          <h3 class="dashboard-detail__title">{props.routine().name}</h3>
        </div>
        <div class="dashboard-detail__pills">
          <span
            class={
              TRIGGER_CLASS[String(props.routine().trigger.type)] ??
              "pill pill--neutral"
            }
          >
            {String(props.routine().trigger.type ?? "manual")}
          </span>
          <span
            class={
              STATUS_CLASS[
                props.routine().enabled
                  ? props.routine().consecutive_failures > 0
                    ? "failing"
                    : "active"
                  : "disabled"
              ] ?? "pill pill--neutral"
            }
          >
            {props.routine().enabled
              ? t("routines-enabled-label")
              : t("routines-disabled-label")}
          </span>
        </div>
      </div>

      <p class="dashboard-detail__body">{props.routine().description}</p>

      <dl class="dashboard-detail__meta-grid">
        <div>
          <dt>{t("routines-meta-last-run")}</dt>
          <dd>
            {formatTimestamp(
              props.routine().last_run_at,
              t("timestamp-pending")
            )}
          </dd>
        </div>
        <div>
          <dt>{t("routines-meta-next-run")}</dt>
          <dd>
            {formatTimestamp(
              props.routine().next_fire_at,
              t("timestamp-pending")
            )}
          </dd>
        </div>
        <div>
          <dt>{t("routines-meta-guardrail")}</dt>
          <dd>{t("page-routines-guardrail")}</dd>
        </div>
      </dl>

      <div class="dashboard-detail__actions">
        <button
          class="dashboard-detail__ghost"
          type="button"
          onClick={() => props.onTrigger()}
        >
          {t("routines-action-run-now")}
        </button>
        <button
          class="dashboard-detail__ghost"
          type="button"
          onClick={() => props.onToggle()}
        >
          {props.routine().enabled
            ? t("routines-action-disable")
            : t("routines-action-enable")}
        </button>
        <button
          class="dashboard-detail__ghost"
          type="button"
          onClick={() => props.onDelete()}
        >
          {t("routines-action-delete")}
        </button>
      </div>

      <section class="catalogue-panel">
        <div class="catalogue-panel__content">
          <h3 class="catalogue-panel__title">{t("routines-runs-title")}</h3>
          <div class="catalogue-list catalogue-list--extensions">
            <For each={props.runs()}>
              {(run) => (
                <article class="catalogue-list__row">
                  <div class="catalogue-list__key">{run.status}</div>
                  <div class="catalogue-list__content">
                    <p class="catalogue-list__source">
                      {formatTimestamp(run.started_at, t("timestamp-pending"))}
                    </p>
                    <p class="catalogue-list__body">
                      {run.result_summary ?? t("routines-run-no-summary")}
                    </p>
                  </div>
                </article>
              )}
            </For>
          </div>
        </div>
      </section>
    </section>
  );
};

export const RoutinesPreview = () => {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const [activeRoutineId, setActiveRoutineId] = createSignal<string>();

  const routines = createQuery(() => ({
    queryKey: ["routines", "list"],
    queryFn: fetchRoutines,
  }));

  const summary = createQuery(() => ({
    queryKey: ["routines", "summary"],
    queryFn: fetchRoutineSummary,
  }));

  createEffect(() => {
    const firstRoutine = routines.data?.routines[0]?.id;
    if (!activeRoutineId() && firstRoutine) {
      setActiveRoutineId(firstRoutine);
    }
  });

  const activeRoutine = createQuery(() => ({
    queryKey: ["routines", "detail", activeRoutineId()],
    queryFn: () => fetchRoutineDetail(activeRoutineId() ?? ""),
    enabled: typeof activeRoutineId() === "string",
    placeholderData: keepPreviousData,
  }));

  const runs = createQuery(() => ({
    queryKey: ["routines", "runs", activeRoutineId()],
    queryFn: () => fetchRoutineRuns(activeRoutineId() ?? ""),
    enabled: typeof activeRoutineId() === "string",
    placeholderData: keepPreviousData,
  }));

  const refresh = () => {
    void queryClient.invalidateQueries({ queryKey: ["routines"] });
  };

  const triggerMutation = createMutation(() => ({
    mutationFn: () => triggerRoutine(activeRoutineId() ?? ""),
    onSuccess: refresh,
  }));

  const toggleMutation = createMutation(() => ({
    mutationFn: () =>
      toggleRoutine(activeRoutineId() ?? "", {
        enabled: !activeRoutine.data?.enabled,
      }),
    onSuccess: refresh,
  }));

  const deleteMutation = createMutation(() => ({
    mutationFn: () => deleteRoutine(activeRoutineId() ?? ""),
    onSuccess: () => {
      refresh();
      setActiveRoutineId(undefined);
    },
  }));

  const summaryCards = createMemo(() => {
    if (!summary.data) {
      return [];
    }
    return [
      { key: "total", value: summary.data.total },
      { key: "enabled", value: summary.data.enabled },
      { key: "disabled", value: summary.data.disabled },
      { key: "failing", value: summary.data.failing },
      { key: "runs_today", value: summary.data.runs_today },
    ];
  });

  return (
    <section class="route-preview route-preview--dashboard">
      <div aria-hidden="true" class="route-preview__watermark">
        {t("routines-watermark")}
      </div>

      <div class="dashboard-preview">
        <header class="route-preview__intro dashboard-preview__intro">
          <h2 class="route-preview__title">{t("route-routines-label")}</h2>
          <p class="route-preview__summary">{t("page-routines-summary")}</p>
        </header>

        <SummaryCards cards={summaryCards} />

        <RoutinesTable
          routines={() => routines.data?.routines ?? []}
          activeRoutineId={activeRoutineId}
          onSelect={(id) => setActiveRoutineId(id)}
        />

        <Show when={activeRoutine.data}>
          {(routine) => (
            <RoutineDetailPanel
              routine={routine}
              runs={() => runs.data?.runs ?? []}
              onTrigger={() => triggerMutation.mutate()}
              onToggle={() => toggleMutation.mutate()}
              onDelete={() => deleteMutation.mutate()}
            />
          )}
        </Show>
      </div>
    </section>
  );
};
