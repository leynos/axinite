import { createMutation, createQuery } from "@tanstack/solid-query";
import type { Accessor } from "solid-js";
import {
  createEffect,
  createMemo,
  createSignal,
  For,
  onCleanup,
  Show,
} from "solid-js";
import type { LogEntry } from "@/lib/api/contracts";
import { connectLogEvents, fetchLogLevel, setLogLevel } from "@/lib/api/logs";
import { useFeatureFlags } from "@/lib/feature-flags/runtime";
import { useI18n } from "@/lib/i18n/provider";

const MAX_RETAINED_ENTRIES = 500;

const LEVEL_SEVERITY: Record<LogEntry["level"], number> = {
  trace: 0,
  debug: 1,
  info: 2,
  warn: 3,
  error: 4,
};

const DISPLAY_LEVELS: LogEntry["level"][] = [
  "trace",
  "debug",
  "info",
  "warn",
  "error",
];

function formatTimestamp(value: string): string {
  return new Intl.DateTimeFormat("en-GB", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  }).format(new Date(value));
}

type Translate = ReturnType<typeof useI18n>["t"];

interface LogsControlsProps {
  t: Translate;
  levelValue: Accessor<string>;
  onLevelChange: (value: string) => void;
  displayLevel: Accessor<LogEntry["level"]>;
  setDisplayLevel: (value: LogEntry["level"]) => void;
  targetFilter: Accessor<string>;
  setTargetFilter: (value: string) => void;
  paused: Accessor<boolean>;
  onTogglePause: () => void;
  onClear: () => void;
  autoScroll: Accessor<boolean>;
  setAutoScroll: (value: boolean) => void;
}

// Narrow-props controls row for the logs stream. Props are accessed lazily
// (never destructured) so SolidJS reactivity is preserved across re-renders.
const LogsControls = (props: LogsControlsProps) => {
  return (
    <div class="catalogue-form">
      <div class="catalogue-form__row">
        <label class="catalogue-form__label" for="logs-level">
          {props.t("logs-level-label")}
        </label>
        <select
          class="catalogue-form__input"
          id="logs-level"
          onChange={(event) => props.onLevelChange(event.currentTarget.value)}
          value={props.levelValue()}
        >
          <option value="debug">{props.t("logs-level-debug")}</option>
          <option value="info">{props.t("logs-level-info")}</option>
          <option value="warn">{props.t("logs-level-warn")}</option>
          <option value="error">{props.t("logs-level-error")}</option>
        </select>
      </div>

      <div class="catalogue-form__row">
        <label class="catalogue-form__label" for="logs-filter-level">
          {props.t("logs-filter-level-label")}
        </label>
        <select
          class="catalogue-form__input"
          id="logs-filter-level"
          onChange={(event) =>
            props.setDisplayLevel(
              event.currentTarget.value as LogEntry["level"]
            )
          }
          value={props.displayLevel()}
        >
          <For each={DISPLAY_LEVELS}>
            {(candidate) => (
              <option value={candidate}>
                {props.t(`logs-level-${candidate}`)}
              </option>
            )}
          </For>
        </select>
      </div>

      <div class="catalogue-form__row">
        <label class="catalogue-form__label" for="logs-filter-target">
          {props.t("logs-filter-target-label")}
        </label>
        <input
          class="catalogue-form__input"
          id="logs-filter-target"
          onInput={(event) => props.setTargetFilter(event.currentTarget.value)}
          type="text"
          value={props.targetFilter()}
        />
      </div>

      <div class="catalogue-form__row">
        <button
          class="btn btn-ghost btn-sm"
          onClick={() => props.onTogglePause()}
          type="button"
        >
          {props.paused() ? props.t("logs-resume") : props.t("logs-pause")}
        </button>
        <button
          class="btn btn-ghost btn-sm"
          onClick={() => props.onClear()}
          type="button"
        >
          {props.t("logs-clear")}
        </button>
        <label class="catalogue-form__label">
          <input
            checked={props.autoScroll()}
            onChange={(event) =>
              props.setAutoScroll(event.currentTarget.checked)
            }
            type="checkbox"
          />
          {props.t("logs-autoscroll")}
        </label>
      </div>
    </div>
  );
};

interface LogsEntryListProps {
  entries: Accessor<LogEntry[]>;
  setRef: (element: HTMLDivElement) => void;
}

// Narrow-props scrollable list of rendered log entries. The `setRef` callback
// hands the host element back to the parent so its autoscroll effect keeps
// working; `entries` stays an accessor to preserve reactivity.
const LogsEntryList = (props: LogsEntryListProps) => {
  return (
    <div
      aria-live="polite"
      class="logs-panel"
      ref={(element) => {
        props.setRef(element);
      }}
    >
      <For each={props.entries()}>
        {(entry) => (
          <article class="logs-panel__item">
            <p class="logs-panel__time">
              {formatTimestamp(entry.timestamp)} · {entry.level}
            </p>
            <p class="logs-panel__message">
              [{entry.target}] {entry.message}
            </p>
          </article>
        )}
      </For>
    </div>
  );
};

const LogsStream = () => {
  const { t } = useI18n();
  const [entries, setEntries] = createSignal<LogEntry[]>([]);
  const [paused, setPaused] = createSignal(false);
  const [displayLevel, setDisplayLevel] =
    createSignal<LogEntry["level"]>("trace");
  const [targetFilter, setTargetFilter] = createSignal("");
  const [autoScroll, setAutoScroll] = createSignal(true);
  let scrollRef: HTMLDivElement | undefined;

  const level = createQuery(() => ({
    queryKey: ["logs", "level"],
    queryFn: fetchLogLevel,
  }));

  const levelMutation = createMutation(() => ({
    mutationFn: (nextLevel: string) => setLogLevel(nextLevel),
  }));

  createEffect(() => {
    const source = connectLogEvents((entry) => {
      if (paused()) {
        return;
      }
      setEntries((current) => [...current, entry].slice(-MAX_RETAINED_ENTRIES));
    });
    onCleanup(() => source.close());
  });

  const filteredEntries = createMemo(() => {
    const threshold = LEVEL_SEVERITY[displayLevel()];
    const target = targetFilter().trim();
    return entries().filter(
      (entry) =>
        LEVEL_SEVERITY[entry.level] >= threshold &&
        (target.length === 0 || entry.target.includes(target))
    );
  });

  createEffect(() => {
    filteredEntries();
    if (autoScroll() && scrollRef) {
      scrollRef.scrollTop = scrollRef.scrollHeight;
    }
  });

  return (
    <section class="dashboard-panel">
      <div class="dashboard-panel__header">
        <div>
          <h3 class="dashboard-panel__title">{t("logs-title")}</h3>
          <p class="dashboard-panel__body">{t("logs-description")}</p>
        </div>
      </div>

      <LogsControls
        autoScroll={autoScroll}
        displayLevel={displayLevel}
        levelValue={() => level.data?.level ?? "info"}
        onClear={() => setEntries([])}
        onLevelChange={(value) => levelMutation.mutate(value)}
        onTogglePause={() => setPaused((current) => !current)}
        paused={paused}
        setAutoScroll={setAutoScroll}
        setDisplayLevel={setDisplayLevel}
        setTargetFilter={setTargetFilter}
        t={t}
        targetFilter={targetFilter}
      />

      <LogsEntryList
        entries={filteredEntries}
        setRef={(element) => {
          scrollRef = element;
        }}
      />
    </section>
  );
};

export const LogsPreview = () => {
  const { t } = useI18n();
  const flags = useFeatureFlags();
  const panelEnabled = createMemo(() => flags.isRouteVisible("panel_logs"));

  return (
    <section class="route-preview route-preview--dashboard">
      <div class="dashboard-preview">
        <header class="route-preview__intro dashboard-preview__intro">
          <h2 class="route-preview__title">{t("route-logs-label")}</h2>
          <p class="route-preview__summary">{t("logs-description")}</p>
        </header>

        <Show
          fallback={
            <div class="route-page__notice">
              <h3>{t("route-unavailable-title")}</h3>
              <p>{t("route-unavailable-body")}</p>
            </div>
          }
          when={panelEnabled()}
        >
          <LogsStream />
        </Show>
      </div>
    </section>
  );
};
