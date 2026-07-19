import {
  createMutation,
  createQuery,
  keepPreviousData,
  useQueryClient,
} from "@tanstack/solid-query";
import { createEffect, createMemo, createSignal, For, Show } from "solid-js";
import type { JobDetailResponse, JobInfo } from "@/lib/api/contracts";
import {
  cancelJob,
  fetchJobDetail,
  fetchJobEvents,
  fetchJobFiles,
  fetchJobSummary,
  fetchJobs,
  promptJob,
  readJobFile,
  restartJob,
} from "@/lib/api/jobs";
import { useI18n } from "@/lib/i18n/provider";
import { pascalCase } from "@/lib/string-case";

const STATUS_CLASS: Record<string, string> = {
  completed: "pill pill--success",
  failed: "pill pill--danger",
  in_progress: "pill pill--success",
  pending: "pill pill--neutral",
  stuck: "pill pill--warning",
};

const SOURCE_CLASS: Record<string, string> = {
  agent: "pill pill--neutral",
  direct: "pill pill--neutral",
  sandbox: "pill pill--info",
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

function sourceName(
  job: JobDetailResponse | JobInfo | { job_kind?: string; job_mode?: string }
): string {
  if ("job_kind" in job && typeof job.job_kind === "string") {
    return job.job_kind;
  }
  if ("job_mode" in job && typeof job.job_mode === "string") {
    return job.job_mode;
  }
  return "direct";
}

export const JobsPreview = () => {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const [activeJobId, setActiveJobId] = createSignal<string>();
  const [activeFilePath, setActiveFilePath] = createSignal<string>();
  const [promptText, setPromptText] = createSignal("");

  const jobs = createQuery(() => ({
    queryKey: ["jobs", "list"],
    queryFn: fetchJobs,
  }));

  const summary = createQuery(() => ({
    queryKey: ["jobs", "summary"],
    queryFn: fetchJobSummary,
  }));

  createEffect(() => {
    const firstJob = jobs.data?.jobs[0]?.id;
    if (!activeJobId() && firstJob) {
      setActiveJobId(firstJob);
    }
  });

  const activeJob = createQuery(() => ({
    queryKey: ["jobs", "detail", activeJobId()],
    queryFn: () => fetchJobDetail(activeJobId() ?? ""),
    enabled: typeof activeJobId() === "string",
    placeholderData: keepPreviousData,
  }));

  const events = createQuery(() => ({
    queryKey: ["jobs", "events", activeJobId()],
    queryFn: () => fetchJobEvents(activeJobId() ?? ""),
    enabled: typeof activeJobId() === "string",
    placeholderData: keepPreviousData,
  }));

  const files = createQuery(() => ({
    queryKey: ["jobs", "files", activeJobId()],
    queryFn: () => fetchJobFiles(activeJobId() ?? ""),
    enabled: typeof activeJobId() === "string",
    placeholderData: keepPreviousData,
  }));

  createEffect(() => {
    const firstFile = files.data?.entries[0]?.path;
    if (firstFile && firstFile !== activeFilePath()) {
      setActiveFilePath(firstFile);
    }
  });

  const fileContent = createQuery(() => ({
    queryKey: ["jobs", "file", activeJobId(), activeFilePath()],
    queryFn: () => readJobFile(activeJobId() ?? "", activeFilePath() ?? ""),
    enabled:
      typeof activeJobId() === "string" && typeof activeFilePath() === "string",
    placeholderData: keepPreviousData,
  }));

  const refreshJobs = () => {
    void queryClient.invalidateQueries({ queryKey: ["jobs"] });
  };

  const restartMutation = createMutation(() => ({
    mutationFn: () => restartJob(activeJobId() ?? ""),
    onSuccess: refreshJobs,
  }));

  const cancelMutation = createMutation(() => ({
    mutationFn: () => cancelJob(activeJobId() ?? ""),
    onSuccess: refreshJobs,
  }));

  const promptMutation = createMutation(() => ({
    mutationFn: () =>
      promptJob(activeJobId() ?? "", {
        prompt: promptText(),
      }),
    onSuccess: () => {
      refreshJobs();
    },
  }));

  const summaryCards = createMemo(() => {
    if (!summary.data) {
      return [];
    }
    return [
      { key: "total", value: summary.data.total },
      { key: "in_progress", value: summary.data.in_progress },
      { key: "completed", value: summary.data.completed },
      { key: "failed", value: summary.data.failed },
      { key: "stuck", value: summary.data.stuck },
    ];
  });

  return (
    <section class="route-preview route-preview--dashboard">
      <div aria-hidden="true" class="route-preview__watermark">
        {t("jobs-watermark")}
      </div>

      <div class="dashboard-preview">
        <header class="route-preview__intro dashboard-preview__intro">
          <h2 class="route-preview__title">{t("route-jobs-label")}</h2>
          <p class="route-preview__summary">{t("page-jobs-summary")}</p>
        </header>

        <div class="dashboard-summary">
          <For each={summaryCards()}>
            {(card) => (
              <article class="dashboard-summary__card">
                <p class="dashboard-summary__label">
                  {t(`jobs-summary-${toKebabSegment(card.key)}`)}
                </p>
                <p class="dashboard-summary__value">{card.value}</p>
              </article>
            )}
          </For>
        </div>

        <section class="dashboard-panel">
          <div class="dashboard-panel__header">
            <div>
              <h3 class="dashboard-panel__title">{t("jobs-table-title")}</h3>
              <p class="dashboard-panel__body">{t("page-jobs-agenda")}</p>
            </div>
          </div>

          <div class="dashboard-table-wrap">
            <table class="dashboard-table">
              <thead>
                <tr>
                  <th>{t("jobs-column-id")}</th>
                  <th>{t("jobs-column-title")}</th>
                  <th>{t("jobs-column-source")}</th>
                  <th>{t("jobs-column-status")}</th>
                  <th>{t("jobs-column-created")}</th>
                  <th>{t("jobs-column-actions")}</th>
                </tr>
              </thead>
              <tbody>
                <For each={jobs.data?.jobs ?? []}>
                  {(job) => (
                    <tr
                      class={
                        activeJobId() === job.id
                          ? "dashboard-table__row dashboard-table__row--active"
                          : "dashboard-table__row"
                      }
                    >
                      <td class="dashboard-table__mono">{job.id}</td>
                      <td>
                        <button
                          class="dashboard-table__title-button"
                          onClick={() => setActiveJobId(job.id)}
                          type="button"
                        >
                          {job.title}
                        </button>
                      </td>
                      <td>
                        <span
                          class={
                            SOURCE_CLASS[sourceName(job)] ??
                            "pill pill--neutral"
                          }
                        >
                          {sourceName(job)}
                        </span>
                      </td>
                      <td>
                        <span
                          class={
                            STATUS_CLASS[job.state] ?? "pill pill--neutral"
                          }
                        >
                          {t(`jobs-status-${toKebabSegment(job.state)}`)}
                        </span>
                      </td>
                      <td class="dashboard-table__meta">
                        {formatTimestamp(
                          job.created_at,
                          t("timestamp-pending")
                        )}
                      </td>
                      <td>
                        <button
                          class="dashboard-table__action"
                          onClick={() => setActiveJobId(job.id)}
                          type="button"
                        >
                          {t("jobs-action-inspect")}
                        </button>
                      </td>
                    </tr>
                  )}
                </For>
              </tbody>
            </table>
          </div>
        </section>

        <Show when={activeJob.data}>
          {(job) => (
            <section class="dashboard-detail">
              <div class="dashboard-detail__header">
                <div>
                  <p class="dashboard-detail__eyebrow">
                    {t("jobs-detail-eyebrow")}
                  </p>
                  <h3 class="dashboard-detail__title">{job().title}</h3>
                </div>
                <div class="dashboard-detail__pills">
                  <span
                    class={
                      SOURCE_CLASS[sourceName(job())] ?? "pill pill--neutral"
                    }
                  >
                    {sourceName(job())}
                  </span>
                  <span
                    class={STATUS_CLASS[job().state] ?? "pill pill--neutral"}
                  >
                    {t(`jobs-status-${toKebabSegment(job().state)}`)}
                  </span>
                </div>
              </div>

              <p class="dashboard-detail__body">{job().description}</p>

              <dl class="dashboard-detail__meta-grid">
                <div>
                  <dt>{t("jobs-meta-created")}</dt>
                  <dd>
                    {formatTimestamp(job().created_at, t("timestamp-pending"))}
                  </dd>
                </div>
                <div>
                  <dt>{t("jobs-meta-elapsed")}</dt>
                  <dd>
                    {job().elapsed_secs
                      ? `${job().elapsed_secs}s`
                      : t("jobs-elapsed-pending")}
                  </dd>
                </div>
                <div>
                  <dt>{t("jobs-meta-guardrail")}</dt>
                  <dd>{t("page-jobs-guardrail")}</dd>
                </div>
              </dl>

              <div class="dashboard-detail__actions">
                <button
                  class="dashboard-detail__ghost"
                  type="button"
                  onClick={() => restartMutation.mutate()}
                  disabled={!job().can_restart}
                >
                  {t("jobs-action-restart")}
                </button>
                <button
                  class="dashboard-detail__ghost"
                  type="button"
                  onClick={() => cancelMutation.mutate()}
                  disabled={
                    job().state !== "in_progress" && job().state !== "pending"
                  }
                >
                  {t("jobs-action-cancel")}
                </button>
              </div>

              <div class="catalogue-panel-grid catalogue-panel-grid--extensions">
                <section class="catalogue-panel">
                  <div class="catalogue-panel__content">
                    <h3 class="catalogue-panel__title">
                      {t("jobs-activity-title")}
                    </h3>
                    <div class="catalogue-list catalogue-list--extensions">
                      <For each={events.data?.events ?? []}>
                        {(event) => (
                          <article class="catalogue-list__row">
                            <div class="catalogue-list__key">{event.level}</div>
                            <div class="catalogue-list__content">
                              <p class="catalogue-list__source">
                                {formatTimestamp(
                                  event.timestamp,
                                  t("timestamp-pending")
                                )}
                              </p>
                              <p class="catalogue-list__body">
                                {event.message}
                              </p>
                            </div>
                          </article>
                        )}
                      </For>
                    </div>
                  </div>
                </section>

                <section class="catalogue-panel">
                  <div class="catalogue-panel__content">
                    <h3 class="catalogue-panel__title">
                      {t("jobs-files-title")}
                    </h3>
                    <div class="catalogue-files skills-detail__files">
                      <div class="catalogue-files__list skills-files__list">
                        <For each={files.data?.entries ?? []}>
                          {(file) => (
                            <button
                              class="catalogue-files__item"
                              onClick={() => setActiveFilePath(file.path)}
                              type="button"
                            >
                              {file.path}
                            </button>
                          )}
                        </For>
                      </div>
                    </div>
                    <Show when={fileContent.data?.content}>
                      <pre class="memory-preview__document">
                        {fileContent.data?.content}
                      </pre>
                    </Show>
                  </div>
                </section>
              </div>

              <div class="chat-preview__composer-wrap">
                <div class="chat-preview__composer-shell">
                  <div class="chat-preview__composer">
                    <textarea
                      aria-label={t("jobs-prompt-label")}
                      class="chat-preview__textarea"
                      onInput={(event) =>
                        setPromptText(event.currentTarget.value)
                      }
                      placeholder={t("jobs-prompt-placeholder")}
                      rows={1}
                      value={promptText()}
                    />
                    <div class="chat-preview__composer-actions">
                      <button
                        class="chat-preview__send-button"
                        disabled={
                          !job().can_prompt || promptText().trim().length === 0
                        }
                        type="button"
                        onClick={() => promptMutation.mutate()}
                      >
                        {t("jobs-prompt-send")}
                      </button>
                    </div>
                  </div>
                  <p class="chat-preview__safety-note">
                    {t("jobs-prompt-safety")}
                  </p>
                </div>
              </div>
            </section>
          )}
        </Show>
      </div>
    </section>
  );
};
