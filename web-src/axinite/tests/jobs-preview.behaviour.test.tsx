import { render, screen, waitFor } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { createSignal } from "solid-js";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { AppProviders } from "@/app/providers";
import { JobDetail } from "@/components/jobs/job-detail";
import { JobsPreview } from "@/components/jobs-preview";
import type {
  ChatSseEvent,
  JobDetailResponse,
  JobEventInfo,
  ProjectFileEntry,
} from "@/lib/api/contracts";
import { promptJob } from "@/lib/api/jobs";
import { DEFAULT_LOCALE } from "@/lib/i18n/supported-locales";
import { setupI18nTestHarness } from "./support/i18n-test-runtime";
import { TestProviders } from "./support/test-providers";

const jobsApiMocks = vi.hoisted(() => ({
  fetchJobs: vi.fn(),
  fetchJobSummary: vi.fn(),
  fetchJobDetail: vi.fn(),
  fetchJobEvents: vi.fn(),
  fetchJobFiles: vi.fn(),
  readJobFile: vi.fn(),
  restartJob: vi.fn(),
  cancelJob: vi.fn(),
  promptJob: vi.fn(),
}));

vi.mock("@/lib/api/jobs", () => jobsApiMocks);

// The live activity source is injected in the detail tests, so the chat module
// is stubbed only to keep the default import resolvable.
vi.mock("@/lib/api/chat", () => ({
  connectChatEvents: vi.fn(() => ({ close: vi.fn() })),
}));

const FLAG_OVERRIDE_KEY = "axinite.feature-flag-overrides";

function makeJob(
  overrides: Partial<JobDetailResponse> = {}
): JobDetailResponse {
  return {
    id: "job-1",
    title: "Test job",
    description: "A test job description",
    state: "in_progress",
    user_id: "mock-user",
    created_at: "2026-07-19T10:00:00Z",
    started_at: "2026-07-19T10:00:00Z",
    completed_at: null,
    elapsed_secs: 12,
    project_dir: "/workspace/axinite",
    browse_url: "https://example.com/projects/job-1/",
    job_mode: "sandbox",
    transitions: [
      {
        from: "queued",
        to: "running",
        timestamp: "2026-07-19T10:00:00Z",
        reason: null,
      },
      {
        from: "running",
        to: "completed",
        timestamp: "2026-07-19T10:05:00Z",
        reason: "Finished cleanly",
      },
    ],
    can_restart: true,
    can_prompt: true,
    job_kind: "sandbox",
    ...overrides,
  };
}

type RenderDetailOptions = {
  events?: JobEventInfo[];
  files?: ProjectFileEntry[];
  onSubmitPrompt?: (done: boolean) => void;
};

type RenderedDetail = {
  emit: (event: ChatSseEvent) => void;
  selectedFile: () => string | undefined;
  promptText: () => string;
};

function renderJobDetail(
  job: JobDetailResponse,
  options: RenderDetailOptions = {}
): RenderedDetail {
  const [activePath, setActivePath] = createSignal<string>();
  const [promptText, setPromptText] = createSignal("");
  const [fileContent, setFileContent] = createSignal<string>();
  let emit: (event: ChatSseEvent) => void = () => undefined;

  const onSelectFile = (path: string) => {
    setActivePath(path);
    setFileContent(`content of ${path}`);
  };

  render(() => (
    <TestProviders>
      <JobDetail
        activePath={activePath}
        connectLive={(onEvent) => {
          emit = onEvent;
          return { close: vi.fn() };
        }}
        events={() => options.events ?? []}
        fileContent={fileContent}
        files={() => options.files ?? []}
        job={() => job}
        onCancel={() => undefined}
        onPromptInput={setPromptText}
        onRestart={() => undefined}
        onSelectFile={onSelectFile}
        onSubmitPrompt={options.onSubmitPrompt ?? (() => undefined)}
        promptText={promptText}
      />
    </TestProviders>
  ));

  return {
    emit: (event) => emit(event),
    selectedFile: activePath,
    promptText,
  };
}

beforeAll(async () => {
  await setupI18nTestHarness();
});

beforeEach(async () => {
  for (const mock of Object.values(jobsApiMocks)) {
    mock.mockReset();
  }
  window.localStorage.clear();
  const runtime = await import("@/lib/i18n/runtime");
  await runtime.default.changeLanguage(DEFAULT_LOCALE);

  jobsApiMocks.fetchJobSummary.mockResolvedValue({
    total: 0,
    running: 0,
    completed: 0,
    failed: 0,
    stuck: 0,
  });
});

describe("jobs preview error handling", () => {
  it("shows a visible error state when the jobs list request fails", async () => {
    jobsApiMocks.fetchJobs.mockRejectedValue(
      new Error("Simulated failure for /api/jobs.")
    );

    render(() => (
      <AppProviders>
        <JobsPreview />
      </AppProviders>
    ));

    await waitFor(
      () => {
        expect(screen.getByRole("alert")).toBeVisible();
        expect(screen.getByRole("alert").textContent).toContain(
          "Jobs could not be loaded"
        );
      },
      { timeout: 5_000 }
    );
  });
});

describe("jobs detail tabs", () => {
  it("switches between the overview and activity panels", async () => {
    renderJobDetail(makeJob(), {
      events: [
        {
          id: "event-1",
          level: "info",
          message: "Persisted activity row",
          timestamp: "2026-07-19T10:01:00Z",
        },
      ],
    });

    expect(
      screen.getByRole("tab", { name: "Overview", selected: true })
    ).toBeInTheDocument();
    // The active-panel content is visible; the inactive panel is hidden.
    expect(screen.getByText("A test job description")).toBeVisible();

    await userEvent.click(screen.getByRole("tab", { name: "Activity" }));
    expect(
      screen.getByRole("tab", { name: "Activity", selected: true })
    ).toBeInTheDocument();
    expect(await screen.findByText("Persisted activity row")).toBeVisible();
  });

  it("renders the transitions timeline from the fixture, oldest first", () => {
    renderJobDetail(makeJob());

    const timeline = screen.getByRole("list");
    const statuses = screen.getAllByText(/^(running|completed)$/);
    expect(statuses.map((node) => node.textContent)).toEqual([
      "running",
      "completed",
    ]);
    expect(timeline).toHaveTextContent("Finished cleanly");
  });

  it("renders a browse link only for http(s) URLs", () => {
    renderJobDetail(makeJob());

    const link = screen.getByRole("link", { name: "Open project browser" });
    expect(link).toHaveAttribute("href", "https://example.com/projects/job-1/");
  });

  it("hides the browse link for non-http URLs", () => {
    renderJobDetail(makeJob({ browse_url: "/projects/job-1/" }));

    expect(
      screen.queryByRole("link", { name: "Open project browser" })
    ).toBeNull();
  });

  it("hides restart and cancel when the restart flag is off", () => {
    renderJobDetail(makeJob());

    expect(screen.queryByRole("button", { name: "Restart" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Cancel" })).toBeNull();
  });

  it("hides restart when the flag is on but the job cannot restart", () => {
    window.localStorage.setItem(
      FLAG_OVERRIDE_KEY,
      JSON.stringify({ action_job_restart: true })
    );
    renderJobDetail(makeJob({ can_restart: false }));

    expect(screen.queryByRole("button", { name: "Restart" })).toBeNull();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeInTheDocument();
  });

  it("shows restart when the flag is on and the job can restart", () => {
    window.localStorage.setItem(
      FLAG_OVERRIDE_KEY,
      JSON.stringify({ action_job_restart: true })
    );
    renderJobDetail(makeJob({ can_restart: true }));

    expect(screen.getByRole("button", { name: "Restart" })).toBeInTheDocument();
  });

  it("appends a live SSE job event to the activity feed", async () => {
    const detail = renderJobDetail(makeJob());

    await userEvent.click(screen.getByRole("tab", { name: "Activity" }));
    detail.emit({
      type: "job_message",
      job_id: "job-1",
      role: "assistant",
      content: "hello from the stream",
    });

    expect(
      await screen.findByText("assistant: hello from the stream")
    ).toBeVisible();
  });

  it("ignores live events for other jobs", async () => {
    const detail = renderJobDetail(makeJob());

    await userEvent.click(screen.getByRole("tab", { name: "Activity" }));
    detail.emit({
      type: "job_message",
      job_id: "some-other-job",
      role: "assistant",
      content: "not for this job",
    });

    await Promise.resolve();
    expect(screen.queryByText(/not for this job/)).toBeNull();
  });

  it("expands a directory in the file tree and opens a file", async () => {
    renderJobDetail(makeJob(), {
      files: [
        { name: "root.md", path: "root.md", is_dir: false },
        { name: "transport.md", path: "notes/transport.md", is_dir: false },
      ],
    });

    await userEvent.click(screen.getByRole("tab", { name: "Files" }));

    // The nested file is hidden until its directory is expanded.
    expect(screen.queryByRole("button", { name: "transport.md" })).toBeNull();

    await userEvent.click(screen.getByRole("button", { name: "notes" }));
    await userEvent.click(
      await screen.findByRole("button", { name: "transport.md" })
    );

    expect(
      await screen.findByText("content of notes/transport.md")
    ).toBeVisible();
  });

  it("posts done:true when the prompt is marked as done", async () => {
    jobsApiMocks.promptJob.mockResolvedValue({ success: true, message: "ok" });
    const detail = renderJobDetail(makeJob(), {
      // Mirror the JobsPreview wiring: the composer's done flag shapes the body.
      onSubmitPrompt: (done) =>
        void promptJob(
          "job-1",
          done
            ? { content: detail.promptText(), done: true }
            : { content: detail.promptText() }
        ),
    });

    await userEvent.type(
      screen.getByRole("textbox", { name: "Follow-up prompt" }),
      "do it"
    );
    await userEvent.click(
      screen.getByRole("checkbox", { name: "Mark as done" })
    );
    await userEvent.click(screen.getByRole("button", { name: "Send" }));

    await waitFor(() => {
      expect(jobsApiMocks.promptJob).toHaveBeenCalledWith("job-1", {
        content: "do it",
        done: true,
      });
    });
  });

  it("hides the done checkbox for non-Claude-Code jobs", () => {
    renderJobDetail(makeJob({ job_kind: "agent" }));

    expect(screen.queryByRole("checkbox", { name: "Mark as done" })).toBeNull();
  });
});
