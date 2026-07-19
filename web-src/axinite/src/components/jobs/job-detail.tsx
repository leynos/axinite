import { Tabs } from "@kobalte/core/tabs";
import type { Accessor } from "solid-js";
import {
  createEffect,
  createMemo,
  createSignal,
  For,
  onCleanup,
  Show,
} from "solid-js";

import { isHttpUrl } from "@/components/chat-cards";
import { FileTree } from "@/components/jobs/file-tree";
import {
  CLAUDE_CODE_JOB_KIND,
  formatTimestamp,
  SOURCE_CLASS,
  STATUS_CLASS,
  sourceName,
  toKebabSegment,
  truncatePreview,
} from "@/components/jobs/format";
import { connectChatEvents } from "@/lib/api/chat";
import type {
  ChatSseEvent,
  JobActivityRow,
  JobDetailResponse,
  JobEventInfo,
  ProjectFileEntry,
} from "@/lib/api/contracts";
import { useFeatureFlags } from "@/lib/feature-flags/runtime";
import { useI18n } from "@/lib/i18n/provider";

/** A live-event subscription abstraction; injectable for tests. */
export type LiveEventSubscription = { close: () => void };
export type ConnectLiveEvents = (
  onEvent: (event: ChatSseEvent) => void
) => LiveEventSubscription;

const KIND_SUFFIX: Record<Exclude<JobActivityRow["kind"], "log">, string> = {
  message: "message",
  tool_use: "tool-use",
  tool_result: "tool-result",
  status: "status",
  result: "result",
};

function defaultConnectLive(
  onEvent: (event: ChatSseEvent) => void
): LiveEventSubscription {
  const source = connectChatEvents(onEvent);
  return { close: () => source.close() };
}

function mapLiveEvent(
  event: ChatSseEvent,
  jobId: string,
  key: string
): JobActivityRow | null {
  if (!("job_id" in event) || event.job_id !== jobId) {
    return null;
  }
  const timestamp = new Date().toISOString();
  switch (event.type) {
    case "job_message":
      return {
        key,
        kind: "message",
        message: `${event.role}: ${event.content}`,
        timestamp,
      };
    case "job_tool_use":
      return {
        key,
        kind: "tool_use",
        message: `${event.tool_name} — ${truncatePreview(
          typeof event.input === "string"
            ? event.input
            : JSON.stringify(event.input ?? {})
        )}`,
        timestamp,
      };
    case "job_tool_result":
      return {
        key,
        kind: "tool_result",
        message: `${event.tool_name} — ${truncatePreview(event.output)}`,
        timestamp,
      };
    case "job_status":
      return { key, kind: "status", message: event.message, timestamp };
    case "job_result":
      return { key, kind: "result", message: event.status, timestamp };
    default:
      return null;
  }
}

function persistedRow(event: JobEventInfo): JobActivityRow {
  return {
    key: event.id,
    kind: "log",
    level: event.level,
    message: event.message,
    timestamp: event.timestamp,
  };
}

export type JobDetailProps = {
  job: Accessor<JobDetailResponse>;
  events: Accessor<JobEventInfo[]>;
  files: Accessor<ProjectFileEntry[]>;
  fileContent: Accessor<string | undefined>;
  activePath: Accessor<string | undefined>;
  onSelectFile: (path: string) => void;
  promptText: Accessor<string>;
  onPromptInput: (value: string) => void;
  onRestart: () => void;
  onCancel: () => void;
  onSubmitPrompt: (done: boolean) => void;
  /** Injectable live-event source; defaults to the global chat SSE stream. */
  connectLive?: ConnectLiveEvents;
};

export const JobDetail = (props: JobDetailProps) => {
  const { t } = useI18n();
  const flags = useFeatureFlags();
  const [markDone, setMarkDone] = createSignal(false);
  const [liveRows, setLiveRows] = createSignal<JobActivityRow[]>([]);

  const jobId = createMemo(() => props.job().id);
  const restartVisible = () => flags.resolvedFlags().action_job_restart;
  const isClaudeCode = () => props.job().job_kind === CLAUDE_CODE_JOB_KIND;

  // Subscribe to the global chat stream while a job is selected, keeping only
  // the live `job_*` events whose `job_id` matches. Re-subscribing on job
  // change clears the previous job's live rows.
  createEffect(() => {
    const id = jobId();
    setLiveRows([]);
    let counter = 0;
    const connect = props.connectLive ?? defaultConnectLive;
    const subscription = connect((event) => {
      const row = mapLiveEvent(event, id, `live-${id}-${counter}`);
      if (row) {
        counter += 1;
        setLiveRows((rows) => [...rows, row]);
      }
    });
    onCleanup(() => subscription.close());
  });

  const activity = createMemo<JobActivityRow[]>(() => {
    const persisted = props
      .events()
      .map(persistedRow)
      .sort((a, b) => (a.timestamp ?? "").localeCompare(b.timestamp ?? ""));
    return [...persisted, ...liveRows()];
  });

  const activityLabel = (row: JobActivityRow): string =>
    row.kind === "log"
      ? (row.level ?? t("jobs-activity-kind-status"))
      : t(`jobs-activity-kind-${KIND_SUFFIX[row.kind]}`);

  const browseUrl = () => {
    const url = props.job().browse_url;
    return url && isHttpUrl(url) ? url : undefined;
  };

  const metaValue = (value: string | null | undefined): string =>
    value && value.length > 0 ? value : t("jobs-meta-unset");

  const promptEmpty = () => props.promptText().trim().length === 0;

  return (
    <>
      <div class="dashboard-detail__header">
        <div>
          <p class="dashboard-detail__eyebrow">{t("jobs-detail-eyebrow")}</p>
          <h3 class="dashboard-detail__title">{props.job().title}</h3>
        </div>
        <div class="dashboard-detail__pills">
          <span
            class={
              SOURCE_CLASS[sourceName(props.job())] ?? "pill pill--neutral"
            }
          >
            {sourceName(props.job())}
          </span>
          <span class={STATUS_CLASS[props.job().state] ?? "pill pill--neutral"}>
            {t(`jobs-status-${toKebabSegment(props.job().state)}`)}
          </span>
        </div>
      </div>

      <Tabs class="jobs-tabs" defaultValue="overview">
        <Tabs.List class="jobs-tabs__list">
          <Tabs.Trigger class="jobs-tabs__trigger" value="overview">
            {t("jobs-tab-overview")}
          </Tabs.Trigger>
          <Tabs.Trigger class="jobs-tabs__trigger" value="activity">
            {t("jobs-tab-activity")}
          </Tabs.Trigger>
          <Tabs.Trigger class="jobs-tabs__trigger" value="files">
            {t("jobs-tab-files")}
          </Tabs.Trigger>
          <Tabs.Indicator class="jobs-tabs__indicator" />
        </Tabs.List>

        <Tabs.Content class="jobs-tabs__content" value="overview">
          <p class="dashboard-detail__body">{props.job().description}</p>

          <dl class="dashboard-detail__meta-grid">
            <div>
              <dt>{t("jobs-meta-created")}</dt>
              <dd>
                {formatTimestamp(
                  props.job().created_at,
                  t("timestamp-pending")
                )}
              </dd>
            </div>
            <div>
              <dt>{t("jobs-meta-elapsed")}</dt>
              <dd>
                {props.job().elapsed_secs
                  ? `${props.job().elapsed_secs}s`
                  : t("jobs-elapsed-pending")}
              </dd>
            </div>
            <div>
              <dt>{t("jobs-meta-mode")}</dt>
              <dd>{metaValue(props.job().job_mode)}</dd>
            </div>
            <div>
              <dt>{t("jobs-meta-kind")}</dt>
              <dd>{metaValue(props.job().job_kind)}</dd>
            </div>
            <div>
              <dt>{t("jobs-meta-project")}</dt>
              <dd>{metaValue(props.job().project_dir)}</dd>
            </div>
            <div>
              <dt>{t("jobs-meta-guardrail")}</dt>
              <dd>{t("page-jobs-guardrail")}</dd>
            </div>
          </dl>

          <section class="jobs-timeline">
            <h4 class="jobs-timeline__title">{t("jobs-transitions-title")}</h4>
            <Show
              when={props.job().transitions.length > 0}
              fallback={
                <p class="jobs-timeline__empty">
                  {t("jobs-transitions-empty")}
                </p>
              }
            >
              <ol class="jobs-timeline__list">
                <For each={props.job().transitions}>
                  {(transition) => (
                    <li class="jobs-timeline__item">
                      <span
                        class={
                          STATUS_CLASS[transition.to] ?? "pill pill--neutral"
                        }
                      >
                        {transition.to}
                      </span>
                      <span class="jobs-timeline__time">
                        {formatTimestamp(
                          transition.timestamp,
                          t("timestamp-pending")
                        )}
                      </span>
                      <Show when={transition.reason}>
                        <span class="jobs-timeline__reason">
                          {transition.reason}
                        </span>
                      </Show>
                    </li>
                  )}
                </For>
              </ol>
            </Show>
          </section>

          <Show when={browseUrl()}>
            {(url) => (
              <a
                class="jobs-detail__browse"
                href={url()}
                rel="noopener noreferrer"
                target="_blank"
              >
                {t("jobs-browse-link")}
              </a>
            )}
          </Show>

          <Show when={restartVisible()}>
            <div class="dashboard-detail__actions">
              <Show when={props.job().can_restart}>
                <button
                  class="dashboard-detail__ghost"
                  onClick={() => props.onRestart()}
                  type="button"
                >
                  {t("jobs-action-restart")}
                </button>
              </Show>
              <button
                class="dashboard-detail__ghost"
                disabled={
                  props.job().state !== "in_progress" &&
                  props.job().state !== "pending"
                }
                onClick={() => props.onCancel()}
                type="button"
              >
                {t("jobs-action-cancel")}
              </button>
            </div>
          </Show>
        </Tabs.Content>

        <Tabs.Content class="jobs-tabs__content" value="activity">
          <div class="catalogue-list catalogue-list--extensions">
            <Show
              when={activity().length > 0}
              fallback={
                <p class="jobs-timeline__empty">{t("jobs-activity-empty")}</p>
              }
            >
              <For each={activity()}>
                {(row) => (
                  <article class="catalogue-list__row">
                    <div class="catalogue-list__key">{activityLabel(row)}</div>
                    <div class="catalogue-list__content">
                      <p class="catalogue-list__source">
                        {formatTimestamp(row.timestamp, t("timestamp-pending"))}
                      </p>
                      <p class="catalogue-list__body">{row.message}</p>
                    </div>
                  </article>
                )}
              </For>
            </Show>
          </div>
        </Tabs.Content>

        <Tabs.Content class="jobs-tabs__content" value="files">
          <div class="catalogue-files skills-detail__files">
            <Show
              when={props.files().length > 0}
              fallback={
                <p class="jobs-timeline__empty">{t("jobs-files-empty")}</p>
              }
            >
              <FileTree
                activePath={props.activePath()}
                entries={props.files()}
                label={t("jobs-file-tree-label")}
                onSelect={props.onSelectFile}
              />
            </Show>
          </div>
          <Show when={props.fileContent()}>
            <pre class="memory-preview__document">{props.fileContent()}</pre>
          </Show>
        </Tabs.Content>
      </Tabs>

      <div class="chat-preview__composer-wrap">
        <div class="chat-preview__composer-shell">
          <div class="chat-preview__composer">
            <textarea
              aria-label={t("jobs-prompt-label")}
              class="chat-preview__textarea"
              onInput={(event) =>
                props.onPromptInput(event.currentTarget.value)
              }
              placeholder={t("jobs-prompt-placeholder")}
              rows={1}
              value={props.promptText()}
            />
            <div class="chat-preview__composer-actions">
              <Show when={isClaudeCode()}>
                <label class="jobs-prompt__done">
                  <input
                    checked={markDone()}
                    onChange={(event) =>
                      setMarkDone(event.currentTarget.checked)
                    }
                    type="checkbox"
                  />
                  {t("jobs-prompt-done-label")}
                </label>
              </Show>
              <button
                class="chat-preview__send-button"
                disabled={!props.job().can_prompt || promptEmpty()}
                onClick={() => props.onSubmitPrompt(markDone())}
                type="button"
              >
                {t("jobs-prompt-send")}
              </button>
            </div>
          </div>
          <p class="chat-preview__safety-note">{t("jobs-prompt-safety")}</p>
        </div>
      </div>
    </>
  );
};
