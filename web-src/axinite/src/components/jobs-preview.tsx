import {
  createMutation,
  createQuery,
  keepPreviousData,
  useQueryClient,
} from "@tanstack/solid-query";
import { createEffect, createMemo, createSignal, For, Show } from "solid-js";
import {
  formatTimestamp,
  SOURCE_CLASS,
  STATUS_CLASS,
  sourceName,
  toKebabSegment,
} from "@/components/jobs/format";
import { JobDetail } from "@/components/jobs/job-detail";
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
    const firstFile = files.data?.entries.find((entry) => !entry.is_dir)?.path;
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
    mutationFn: (done: boolean) =>
      promptJob(
        activeJobId() ?? "",
        done ? { content: promptText(), done: true } : { content: promptText() }
      ),
    onSuccess: () => {
      setPromptText("");
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

          <Show when={jobs.isError}>
            <p class="route-page__notice" role="alert">
              {t("jobs-load-error")}
            </p>
          </Show>
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
              <JobDetail
                activePath={activeFilePath}
                events={() => events.data?.events ?? []}
                fileContent={() => fileContent.data?.content}
                files={() => files.data?.entries ?? []}
                job={job}
                onCancel={() => cancelMutation.mutate()}
                onPromptInput={setPromptText}
                onRestart={() => restartMutation.mutate()}
                onSelectFile={setActiveFilePath}
                onSubmitPrompt={(done) => promptMutation.mutate(done)}
                promptText={promptText}
              />
            </section>
          )}
        </Show>
      </div>
    </section>
  );
};
