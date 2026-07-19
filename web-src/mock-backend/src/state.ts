import type {
  ActionResponse,
  ApprovalRequest,
  CatalogSkillEntry,
  ChatSseEvent,
  ExtensionInfo,
  ExtensionSetupResponse,
  FeatureFlagsResponse,
  GatewayStatusResponse,
  HistoryResponse,
  JobDetailResponse,
  JobEventInfo,
  JobEventsResponse,
  JobInfo,
  JobListResponse,
  JobPromptRequest,
  JobSummaryResponse,
  LogEntry,
  LogLevelResponse,
  MemoryListResponse,
  MemoryReadResponse,
  MemorySearchRequest,
  MemorySearchResponse,
  MemoryTreeResponse,
  MemoryWriteRequest,
  MemoryWriteResponse,
  PendingApprovalInfo,
  ProjectFileReadResponse,
  ProjectFilesResponse,
  RegistryEntryInfo,
  RegistrySearchResponse,
  RoutineDetailResponse,
  RoutineInfo,
  RoutineListResponse,
  RoutineRunsResponse,
  RoutineSummaryResponse,
  SearchHit,
  SecretFieldInfo,
  SendMessageRequest,
  SendMessageResponse,
  SkillInfo,
  SkillInstallRequest,
  SkillListResponse,
  SkillSearchRequest,
  SkillSearchResponse,
  ThreadInfo,
  ThreadListResponse,
  ToggleRequest,
  ToolInfo,
  ToolListResponse,
  TransitionInfo,
  TurnInfo,
} from "../../axinite/src/lib/api/contracts";

type MemoryDocument = {
  content: string;
  updated_at: string;
};

type MockThread = {
  info: ThreadInfo;
  turns: TurnInfo[];
  pendingApproval?: PendingApprovalInfo;
};

type MockJob = {
  detail: JobDetailResponse;
  events: JobEventInfo[];
  files: Record<string, string>;
};

type MockRoutine = {
  detail: RoutineDetailResponse;
};

type MockExtension = {
  info: ExtensionInfo;
  setupSecrets: SecretFieldInfo[];
};

type MockCatalogSkill = CatalogSkillEntry;

type EventSubscriber<T> = {
  send: (payload: T) => void;
  close: () => void;
};

const DAY_MS = 24 * 60 * 60 * 1000;
const START_TIME = new Date("2026-03-26T11:30:00.000Z");

function iso(minutesAgo = 0): string {
  return new Date(START_TIME.getTime() - minutesAgo * 60 * 1000).toISOString();
}

function nowIso(): string {
  return new Date().toISOString();
}

function withUpdatedThread(
  thread: MockThread,
  updates: Partial<ThreadInfo>
): MockThread {
  return {
    ...thread,
    info: {
      ...thread.info,
      ...updates,
    },
  };
}

function createActionResponse(
  message: string,
  overrides: Partial<ActionResponse> = {}
): ActionResponse {
  return {
    success: true,
    message,
    ...overrides,
  };
}

function statusFromRoutine(detail: RoutineDetailResponse): string {
  if (!detail.enabled) {
    return "disabled";
  }
  if (detail.consecutive_failures > 0) {
    return "failing";
  }
  return "active";
}

export class MockBackendState {
  private readonly bootTime = Date.now();

  private nextCounter = 1;

  private readonly featureFlags: FeatureFlagsResponse = {
    deployment: "mock-preview",
    fetched_at: nowIso(),
    flags: {
      route_chat: true,
      route_memory: true,
      route_jobs: true,
      route_routines: true,
      route_extensions: true,
      route_skills: true,
      panel_logs: true,
    },
  };

  private logLevel = "info";

  private readonly chatSubscribers = new Set<EventSubscriber<ChatSseEvent>>();

  private readonly logSubscribers = new Set<EventSubscriber<LogEntry>>();

  private readonly logs: LogEntry[] = [
    {
      id: "log-boot",
      level: "info",
      message: "Mock gateway booted and attached to the static preview.",
      source: "gateway",
      timestamp: iso(35),
    },
    {
      id: "log-flags",
      level: "debug",
      message: "Runtime feature flags resolved for the mock preview shell.",
      source: "gateway",
      timestamp: iso(24),
    },
    {
      id: "log-locale",
      level: "info",
      message: "Locale bundles loaded successfully for en-GB.",
      source: "i18n",
      timestamp: iso(16),
    },
    {
      id: "log-query",
      level: "info",
      message: "Initial route queries completed against the mock backend.",
      source: "frontend",
      timestamp: iso(8),
    },
  ];

  private activeThreadId = "thread-review";

  private readonly threads = new Map<string, MockThread>([
    [
      "thread-assistant",
      {
        info: {
          id: "thread-assistant",
          state: "Idle",
          turn_count: 3,
          created_at: iso(420),
          updated_at: iso(8),
          title: "Assistant",
          thread_type: "assistant",
          channel: "gateway",
        },
        turns: [
          {
            turn_number: 1,
            user_input:
              "Do you have access to the Firecrawl MCP? Test it by searching for stories about robot pandas.",
            response:
              "Yes, I **do have access to Firecrawl** and the instructions are sufficient for normal use.\n\n### What I did\n1. Used **Firecrawl search** for `robot pandas stories`\n2. Got back plausible results from both **web** and **news**\n3. Used **Firecrawl scrape** on the best result to verify content extraction\n\n### Test result\nThe best hit was a **Forbes** article — *\"This Panda Robot Is Designed To Provide Emotional Support For Seniors\"*. Firecrawl successfully scraped the article about **AnAn**, a panda-cub robot from **Mind with Heart Robotics**.\n\n### Bottom line\n- **Access to Firecrawl:** yes\n- **Able to use it successfully:** yes\n- **Instructions sufficient:** yes, for practical use",
            state: "Completed",
            started_at: iso(418),
            completed_at: iso(416),
            tool_calls: [
              {
                name: "memory_search",
                has_result: true,
                has_error: false,
                result_preview: "No relevant memories found.",
              },
              {
                name: "time",
                has_result: true,
                has_error: false,
                result_preview:
                  '{"iso":"2026-03-26T11:12:00+00:00","timezone":"UTC"}',
              },
              {
                name: "firecrawl_firecrawl_search",
                has_result: true,
                has_error: false,
                result_preview:
                  '{"web":[{"url":"https://www.forbes.com/...","title":"This Panda Robot Is Designed To Provide Emotional Support..."}]}',
              },
              {
                name: "firecrawl_firecrawl_scrape",
                has_result: true,
                has_error: false,
                result_preview:
                  '{"markdown":"# This Panda Robot Is Designed To Provide Emotional Support For Seniors\\n\\nBy Prakhar Khanna..."}',
              },
              {
                name: "memory_write",
                has_result: true,
                has_error: false,
                result_preview:
                  '{"path":"daily/2026-03-26.md","status":"written"}',
              },
            ],
          },
          {
            turn_number: 2,
            user_input:
              "Excellent. The improvements to our fork of Ironclaw are working. The fork is named Axinite.",
            response:
              "That is an excellent name.\n\n**Axinite** sounds sharp, mineral, slightly dangerous, and plausibly capable of puncturing nonsense on contact. Good branding substrate.\n\nI\u2019ve noted:\n- your **Ironclaw fork is named Axinite**\n- the recent **improvements are working**\n\nMy strongest recommendation: **do a regression sweep while the changes are fresh**. That catches sneaky breakage before it grows teeth.",
            state: "Completed",
            started_at: iso(34),
            completed_at: iso(32),
            tool_calls: [
              {
                name: "memory_search",
                has_result: true,
                has_error: false,
              },
              {
                name: "memory_write",
                has_result: true,
                has_error: false,
                result_preview:
                  '{"path":"daily/2026-03-26.md","status":"written"}',
              },
            ],
          },
          {
            turn_number: 3,
            user_input:
              "We haven\u2019t done any work on the WASM catalogue yet. That\u2019s the next step. I\u2019ve also been working on a SolidJS front end.",
            response:
              "So the current shape is:\n\n- **Axinite** improvements are landing\n- **WASM catalogue** is the next untouched chunk\n- You\u2019re building a **SolidJS front end**\n\n## My read\n\nYou\u2019re at the point where **catalogue design and front-end design can accidentally drift apart** if they don\u2019t share one canonical model soon.\n\n## Strong recommendation\n\nBefore implementing much more, define the **minimum viable catalogue contract**. Not the whole cathedral. Just the spine:\n\n1. **What is a catalogue entry?** (tool, channel, MCP server, skill, package?)\n2. **What fields are canonical?** (id, kind, name, version, auth, trust)\n3. **What lifecycle states exist?** (discovered, installed, authenticated, active)\n4. **What is the front end actually browsing?** (raw registry records? computed install status?)\n\nIf those are crisp, the SolidJS layer can be elegant instead of becoming a haunted forest of special cases.",
            state: "Completed",
            started_at: iso(12),
            completed_at: iso(8),
            tool_calls: [
              {
                name: "memory_search",
                has_result: true,
                has_error: false,
              },
              {
                name: "memory_write",
                has_result: true,
                has_error: false,
                result_preview:
                  '{"path":"daily/2026-03-26.md","status":"written"}',
              },
            ],
          },
        ],
      },
    ],
    [
      "thread-review",
      {
        info: {
          id: "thread-review",
          state: "AwaitingApproval",
          turn_count: 2,
          created_at: iso(300),
          updated_at: iso(14),
          title: "Review follow-up",
          thread_type: "thread",
          channel: "gateway",
        },
        turns: [
          {
            turn_number: 1,
            user_input: "Review the mock backend route plan.",
            response:
              "I found a transport risk around SSE proxying and a data-source migration dependency.\n\nThe main areas to address:\n1. **SSE proxy lifetime** \u2014 the preview server must keep connections alive\n2. **JSON contract alignment** \u2014 mock DTOs must match Rust `types.rs`\n3. **Thread state machine** \u2014 states need to match the backend enum exactly",
            state: "Completed",
            started_at: iso(295),
            completed_at: iso(294),
            tool_calls: [
              {
                name: "memory_read",
                has_result: true,
                has_error: false,
                result_preview:
                  '{"path":"workspace/AGENTS.md","content":"# Workspace Instructions..."}',
              },
            ],
          },
          {
            turn_number: 2,
            user_input: "Apply the approved mock-backend implementation plan.",
            response: null,
            state: "AwaitingApproval",
            started_at: iso(18),
            completed_at: null,
            tool_calls: [
              {
                name: "write_files",
                has_result: false,
                has_error: false,
              },
            ],
          },
        ],
        pendingApproval: {
          request_id: "approval-review-1",
          tool_name: "write_files",
          description:
            "Update source files for the approved mock backend rollout.",
          parameters:
            '{"paths":["mock-backend/src/state.ts","axinite/src/lib/api/contracts.ts","axinite/src/components/chat-preview.tsx"]}',
        },
      },
    ],
    [
      "thread-scrape",
      {
        info: {
          id: "thread-scrape",
          state: "Idle",
          turn_count: 2,
          created_at: iso(1_440),
          updated_at: iso(1_120),
          title: "Documentation scrape",
          thread_type: "thread",
          channel: "gateway",
        },
        turns: [
          {
            turn_number: 1,
            user_input: "Find the browser-facing Rust gateway endpoints.",
            response:
              "The main browser routes include:\n\n| Area | Endpoints |\n|------|----------|\n| Chat | `/api/chat/send`, `/api/chat/threads`, `/api/chat/history`, `/api/chat/events` |\n| Memory | `/api/memory/tree`, `/api/memory/list`, `/api/memory/read`, `/api/memory/write` |\n| Jobs | `/api/jobs`, `/api/jobs/:id`, `/api/jobs/:id/events` |\n| Routines | `/api/routines`, `/api/routines/:id` |\n| Extensions | `/api/extensions`, `/api/extensions/:name/setup` |\n| Skills | `/api/skills`, `/api/skills/search` |",
            state: "Completed",
            started_at: iso(1_438),
            completed_at: iso(1_437),
            tool_calls: [
              {
                name: "firecrawl_firecrawl_scrape",
                has_result: true,
                has_error: false,
                result_preview:
                  '{"markdown":"# Axinite Gateway API\\n\\n43 endpoints documented..."}',
              },
            ],
          },
          {
            turn_number: 2,
            user_input: "Extract the route list for the mock server.",
            response:
              "Forty-three endpoints are in scope for the in-memory Bun gateway, plus logs and gateway status support.",
            state: "Completed",
            started_at: iso(1_122),
            completed_at: iso(1_120),
            tool_calls: [],
          },
        ],
      },
    ],
  ]);

  private readonly memoryDocuments = new Map<string, MemoryDocument>([
    [
      "workspace/AGENTS.md",
      {
        content: `# Workspace Instructions\n\nMeasure twice, cut once. Gate each commit. Keep the prototype visually stable while the data layer changes beneath it.`,
        updated_at: iso(90),
      },
    ],
    [
      "workspace/daily/HEARTBEAT.md",
      {
        content: `# Heartbeat\n\n- Static preview served from dist\n- Mock API reachable behind one browser origin\n- SSE stream alive for chat and logs`,
        updated_at: iso(75),
      },
    ],
    [
      "workspace/daily/IDENTITY.md",
      {
        content: `# Identity\n\nAxinite is a Rust-based autonomous agent with a browser gateway and a static preview shell used for route and workflow validation.`,
        updated_at: iso(68),
      },
    ],
    [
      "workspace/daily/MEMORY.md",
      {
        content: `# Memory\n\nKeep route data and mutating interactions backend-driven so the preview behaves like the real browser shell instead of a frozen design comp.`,
        updated_at: iso(50),
      },
    ],
    [
      "workspace/skills/TOOLS.md",
      {
        content: `# Tools\n\n- firecrawl\n- playwright\n- css-view\n- grepai\n- leta`,
        updated_at: iso(43),
      },
    ],
    [
      "workspace/skills/USER.md",
      {
        content: `# User Preferences\n\nThe user expects verify-first remediation, exact gate status, and clean static-preview realism without drifting from the approved route structure.`,
        updated_at: iso(38),
      },
    ],
  ]);

  private readonly jobs = new Map<string, MockJob>([
    [
      "job-audit",
      {
        detail: {
          id: "job-audit",
          title: "Route audit for shell parity",
          description:
            "Compared the current Solid preview routes with the Rust browser architecture document and recorded missing runtime data surfaces.",
          state: "completed",
          user_id: "mock-user",
          created_at: iso(220),
          started_at: iso(219),
          completed_at: iso(210),
          elapsed_secs: 540,
          project_dir: "/workspace/axinite-mockup",
          browse_url: "/axinite-mockup/jobs",
          job_mode: "direct",
          transitions: [
            { from: "queued", to: "running", timestamp: iso(219), reason: null },
            {
              from: "running",
              to: "completed",
              timestamp: iso(210),
              reason: "Audit finished cleanly.",
            },
          ],
          can_restart: true,
          can_prompt: false,
          job_kind: "agent",
        },
        events: [
          {
            id: "job-audit-event-1",
            level: "info",
            message: "Loaded route inventory and upstream contract references.",
            timestamp: iso(218),
          },
          {
            id: "job-audit-event-2",
            level: "info",
            message: "Produced an implementation slice with static-preview transport as the first milestone.",
            timestamp: iso(211),
          },
        ],
        files: {
          "reports/route-audit.md":
            "# Route Audit\n\nAll preview routes still needed backend-backed data before this rollout.\n",
        },
      },
    ],
    [
      "job-comparison",
      {
        detail: {
          id: "job-comparison",
          title: "Compare Bun mock transport options",
          description:
            "Evaluates `http-server` proxying against a single-origin fallback while preserving the built static artefacts.",
          state: "in_progress",
          user_id: "mock-user",
          created_at: iso(130),
          started_at: iso(126),
          completed_at: null,
          elapsed_secs: 1_260,
          project_dir: "/workspace/axinite-mockup",
          browse_url: "/axinite-mockup/jobs",
          job_mode: "direct",
          transitions: [
            { from: "queued", to: "running", timestamp: iso(126), reason: null },
          ],
          can_restart: false,
          can_prompt: true,
          job_kind: "agent",
        },
        events: [
          {
            id: "job-comparison-event-1",
            level: "info",
            message: "Proxying JSON requests through the static preview is green.",
            timestamp: iso(124),
          },
          {
            id: "job-comparison-event-2",
            level: "warn",
            message: "SSE validation is still in progress for the preview server.",
            timestamp: iso(10),
          },
        ],
        files: {
          "notes/transport.md":
            "# Transport Notes\n\n- JSON proxy path works\n- SSE path must stay unbuffered\n",
        },
      },
    ],
    [
      "job-oauth",
      {
        detail: {
          id: "job-oauth",
          title: "Extension auth pairing regression",
          description:
            "Investigates why manual token flows were not reflected back into the preview shell after setup.",
          state: "failed",
          user_id: "mock-user",
          created_at: iso(310),
          started_at: iso(308),
          completed_at: iso(300),
          elapsed_secs: 480,
          project_dir: "/workspace/axinite-mockup",
          browse_url: "/axinite-mockup/extensions",
          job_mode: "sandbox",
          transitions: [
            { from: "queued", to: "running", timestamp: iso(308), reason: null },
            {
              from: "running",
              to: "failed",
              timestamp: iso(300),
              reason: "Mock token callback was never submitted.",
            },
          ],
          can_restart: true,
          can_prompt: true,
          job_kind: "sandbox",
        },
        events: [
          {
            id: "job-oauth-event-1",
            level: "error",
            message: "Manual token was required but no follow-up prompt reached the extension UI.",
            timestamp: iso(300),
          },
        ],
        files: {
          "logs/auth.txt": "Awaiting token for github extension\n",
        },
      },
    ],
    [
      "job-docs",
      {
        detail: {
          id: "job-docs",
          title: "Documentation parity sweep",
          description:
            "Keeps docs, route copy, and execplan progress aligned during the mock backend rollout.",
          state: "stuck",
          user_id: "mock-user",
          created_at: iso(400),
          started_at: iso(395),
          completed_at: null,
          elapsed_secs: 7_200,
          project_dir: "/workspace/axinite-mockup",
          browse_url: "/axinite-mockup/skills",
          job_mode: "direct",
          transitions: [
            { from: "queued", to: "running", timestamp: iso(395), reason: null },
            {
              from: "running",
              to: "stuck",
              timestamp: iso(250),
              reason: "Waiting for approved implementation scope.",
            },
          ],
          can_restart: true,
          can_prompt: true,
          job_kind: "agent",
        },
        events: [
          {
            id: "job-docs-event-1",
            level: "warn",
            message: "Implementation work was blocked until the execplan moved out of draft-only state.",
            timestamp: iso(250),
          },
        ],
        files: {
          "docs/execplans/mock-backend.md":
            "# Mock Backend\n\nImplementation resumed after explicit approval.\n",
        },
      },
    ],
    [
      "job-security",
      {
        detail: {
          id: "job-security",
          title: "Feature-flag override review",
          description:
            "Checks that backend defaults and local debug overrides continue to merge in the right order.",
          state: "pending",
          user_id: "mock-user",
          created_at: iso(25),
          started_at: null,
          completed_at: null,
          elapsed_secs: null,
          project_dir: "/workspace/axinite-mockup",
          browse_url: "/axinite-mockup/chat",
          job_mode: "direct",
          transitions: [
            {
              from: "created",
              to: "queued",
              timestamp: iso(25),
              reason: "Awaiting available worker slot.",
            },
          ],
          can_restart: false,
          can_prompt: false,
          job_kind: "agent",
        },
        events: [
          {
            id: "job-security-event-1",
            level: "info",
            message: "Job is queued behind the current transport validation run.",
            timestamp: iso(24),
          },
        ],
        files: {},
      },
    ],
  ]);

  private readonly routines = new Map<string, MockRoutine>([
    [
      "routine-standup",
      {
        detail: {
          id: "routine-standup",
          name: "Daily standup digest",
          description:
            "Collects route changes, open jobs, and flag overrides into a short morning summary.",
          enabled: true,
          trigger: { type: "cron", schedule: "0 9 * * 1-5" },
          action: { type: "lightweight", prompt: "Summarise active preview work" },
          guardrails: { approvals_required: false },
          notify: { channel: "chat" },
          last_run_at: iso(150),
          next_fire_at: iso(-1_140),
          run_count: 47,
          consecutive_failures: 0,
          created_at: iso(14 * 24 * 60),
          recent_runs: [
            {
              id: "routine-run-standup-1",
              trigger_type: "cron",
              started_at: iso(150),
              completed_at: iso(149),
              status: "completed",
              result_summary: "Published the morning summary to the assistant thread.",
              tokens_used: 812,
              job_id: null,
            },
          ],
        },
      },
    ],
    [
      "routine-deploy",
      {
        detail: {
          id: "routine-deploy",
          name: "Deploy readiness check",
          description:
            "Confirms static build output, preview routes, and contract fixtures before a demo share.",
          enabled: true,
          trigger: { type: "event", pattern: "preview:ready", channel: "gateway" },
          action: { type: "full_job", template: "deploy-check" },
          guardrails: { approvals_required: true },
          notify: { channel: "logs" },
          last_run_at: iso(1_100),
          next_fire_at: null,
          run_count: 12,
          consecutive_failures: 0,
          created_at: iso(30 * 24 * 60),
          recent_runs: [
            {
              id: "routine-run-deploy-1",
              trigger_type: "event",
              started_at: iso(1_100),
              completed_at: iso(1_095),
              status: "completed",
              result_summary: "Preview checks passed for the last demo build.",
              tokens_used: 2_140,
              job_id: "job-audit",
            },
          ],
        },
      },
    ],
    [
      "routine-triage",
      {
        detail: {
          id: "routine-triage",
          name: "Issue triage sweep",
          description:
            "Scans route regressions and turns them into job prompts for follow-up work.",
          enabled: true,
          trigger: { type: "system_event", source: "preview", event_type: "warning" },
          action: { type: "lightweight", prompt: "Summarise new warnings" },
          guardrails: { approvals_required: false },
          notify: { channel: "chat" },
          last_run_at: iso(85),
          next_fire_at: null,
          run_count: 89,
          consecutive_failures: 2,
          created_at: iso(60 * 24 * 60),
          recent_runs: [
            {
              id: "routine-run-triage-1",
              trigger_type: "system_event",
              started_at: iso(85),
              completed_at: iso(84),
              status: "failed",
              result_summary: "Last run stalled waiting for a missing extension setup token.",
              tokens_used: 341,
              job_id: null,
            },
          ],
        },
      },
    ],
    [
      "routine-weekly",
      {
        detail: {
          id: "routine-weekly",
          name: "Weekly architecture report",
          description:
            "Produces a broader report across transport, route parity, and documentation drift.",
          enabled: false,
          trigger: { type: "cron", schedule: "0 8 * * MON" },
          action: { type: "full_job", template: "weekly-report" },
          guardrails: { approvals_required: true },
          notify: { channel: "logs" },
          last_run_at: iso(31 * 24 * 60),
          next_fire_at: null,
          run_count: 15,
          consecutive_failures: 0,
          created_at: iso(90 * 24 * 60),
          recent_runs: [
            {
              id: "routine-run-weekly-1",
              trigger_type: "cron",
              started_at: iso(31 * 24 * 60),
              completed_at: iso(31 * 24 * 60 - 5),
              status: "completed",
              result_summary: "Paused after the last approved report cycle.",
              tokens_used: 4_108,
              job_id: "job-docs",
            },
          ],
        },
      },
    ],
    [
      "routine-health",
      {
        detail: {
          id: "routine-health",
          name: "Preview health ping",
          description:
            "Checks that the built site, mock backend, and SSE streams are still alive for local previewing.",
          enabled: true,
          trigger: { type: "manual" },
          action: { type: "lightweight", prompt: "Verify the local preview stack" },
          guardrails: { approvals_required: false },
          notify: { channel: "logs" },
          last_run_at: iso(540),
          next_fire_at: null,
          run_count: 6,
          consecutive_failures: 0,
          created_at: iso(10 * DAY_MS / (60 * 1000)),
          recent_runs: [
            {
              id: "routine-run-health-1",
              trigger_type: "manual",
              started_at: iso(540),
              completed_at: iso(539),
              status: "completed",
              result_summary: "Static server and SSE probes both responded.",
              tokens_used: 78,
              job_id: null,
            },
          ],
        },
      },
    ],
  ]);

  private readonly extensions = new Map<string, MockExtension>([
    [
      "firecrawl",
      {
        info: {
          name: "firecrawl",
          display_name: "Firecrawl",
          kind: "mcp_server",
          description:
            "Remote browsing and extraction tools for live web research within the preview shell.",
          url: "https://firecrawl.dev",
          authenticated: true,
          active: true,
          tools: ["scrape", "crawl", "firecrawl_browser_session_create"],
          needs_setup: false,
          has_auth: false,
          activation_status: "active",
          version: "3.1.0",
        },
        setupSecrets: [],
      },
    ],
    [
      "github",
      {
        info: {
          name: "github",
          display_name: "GitHub",
          kind: "wasm_tool",
          description:
            "Repository and pull-request operations backed by a mock token-auth flow.",
          url: "https://github.com",
          authenticated: false,
          active: false,
          tools: ["get-pr", "get-project", "create-comment"],
          needs_setup: true,
          has_auth: true,
          activation_status: "configured",
          version: "0.1.3",
        },
        setupSecrets: [
          {
            name: "token",
            prompt: "GitHub personal access token",
            optional: false,
            provided: false,
            auto_generate: false,
          },
        ],
      },
    ],
    [
      "jmap",
      {
        info: {
          name: "jmap",
          display_name: "JMAP Mail",
          kind: "wasm_tool",
          description:
            "Mailbox reads and message triage surfaces used to prove extension setup UI paths.",
          url: "https://jmap.io",
          authenticated: true,
          active: true,
          tools: ["list_mailboxes", "search_messages"],
          needs_setup: true,
          has_auth: false,
          activation_status: "active",
          version: "0.1.0",
        },
        setupSecrets: [
          {
            name: "username",
            prompt: "Mailbox username",
            optional: false,
            provided: true,
            auto_generate: false,
          },
          {
            name: "app_password",
            prompt: "Application password",
            optional: false,
            provided: true,
            auto_generate: false,
          },
        ],
      },
    ],
    [
      "telegram",
      {
        info: {
          name: "telegram",
          display_name: "Telegram",
          kind: "wasm_channel",
          description:
            "Event-driven notification transport with pairing, trigger, and auth state changes.",
          url: "https://telegram.org",
          authenticated: true,
          active: true,
          tools: ["list_chats", "send_message", "watch_updates"],
          needs_setup: true,
          has_auth: false,
          activation_status: "active",
          version: "0.2.3",
        },
        setupSecrets: [
          {
            name: "bot_token",
            prompt: "Telegram bot token",
            optional: false,
            provided: true,
            auto_generate: false,
          },
        ],
      },
    ],
  ]);

  private readonly registryEntries = new Map<string, RegistryEntryInfo>([
    [
      "firecrawl",
      {
        name: "firecrawl",
        display_name: "Firecrawl",
        kind: "mcp_server",
        description:
          "Remote browsing, scraping, crawling, and browser session tools.",
        keywords: ["web", "browser", "scrape", "research"],
        installed: true,
        version: "3.1.0",
      },
    ],
    [
      "github",
      {
        name: "github",
        display_name: "GitHub",
        kind: "wasm_tool",
        description: "Repository, pull request, and issue tools.",
        keywords: ["git", "pr", "repo"],
        installed: true,
        version: "0.1.3",
      },
    ],
    [
      "slack",
      {
        name: "slack",
        display_name: "Slack",
        kind: "wasm_tool",
        description: "Workspace messaging, channels, and notification tools.",
        keywords: ["chat", "messaging", "alerts"],
        installed: false,
        version: "0.4.0",
      },
    ],
  ]);

  private readonly skills = new Map<string, SkillInfo>([
    [
      "rust_ownership",
      {
        name: "rust_ownership",
        description:
          "Ownership and borrowing guidance for Rust code review and refactoring.",
        version: "1.0.0",
        trust: "trusted",
        source: "bundle",
        keywords: ["rust", "ownership", "borrowing"],
      },
    ],
    [
      "openapi_reference",
      {
        name: "openapi_reference",
        description:
          "Reusable patterns for contract-first API documentation and review.",
        version: "2.1.0",
        trust: "trusted",
        source: "bundle",
        keywords: ["openapi", "api", "contracts"],
      },
    ],
    [
      "code_review",
      {
        name: "code_review",
        description:
          "Bug-first review guidance with explicit findings, risks, and missing tests.",
        version: "1.2.0",
        trust: "trusted",
        source: "bundle",
        keywords: ["review", "bugs", "tests"],
      },
    ],
    [
      "frontend_a11y",
      {
        name: "frontend_a11y",
        description:
          "Accessibility guidance for semantic HTML, focus management, and screen-reader parity.",
        version: "0.9.0",
        trust: "preview",
        source: "bundle",
        keywords: ["frontend", "accessibility", "a11y"],
      },
    ],
  ]);

  private readonly catalogSkills = new Map<string, MockCatalogSkill>([
    [
      "react-patterns",
      {
        slug: "catalog/react-patterns",
        name: "react_patterns",
        description:
          "Migration guidance for React screen patterns and data-backed component behaviour.",
        version: "1.3.0",
        score: 0.91,
        updatedAt: iso(600),
        stars: 213,
        downloads: 8_412,
        owner: "catalog",
        keywords: ["react", "patterns", "frontend"],
      },
    ],
    [
      "python-typing",
      {
        slug: "catalog/python-typing",
        name: "python_typing",
        description:
          "Type-checking and runtime validation patterns for Python service code.",
        version: "2.0.0",
        score: 0.77,
        updatedAt: iso(1_100),
        stars: 89,
        downloads: 2_145,
        owner: "catalog",
        keywords: ["python", "typing", "mypy"],
      },
    ],
    [
      "docker-compose",
      {
        slug: "catalog/docker-compose",
        name: "docker_compose",
        description:
          "Local stack composition patterns for browser, API, and worker services.",
        version: "1.1.0",
        score: 0.83,
        updatedAt: iso(720),
        stars: 154,
        downloads: 5_019,
        owner: "catalog",
        keywords: ["docker", "compose", "stack"],
      },
    ],
  ]);

  private nextId(prefix: string): string {
    const value = `${prefix}-${this.nextCounter}`;
    this.nextCounter += 1;
    return value;
  }

  getGatewayStatus(): GatewayStatusResponse {
    const sse_connections = this.chatSubscribers.size;
    const ws_connections = 0;
    return {
      version: "0.0.0-mock",
      sse_connections,
      ws_connections,
      total_connections: sse_connections + ws_connections,
      uptime_secs: Math.floor((Date.now() - this.bootTime) / 1_000),
      restart_enabled: false,
      daily_cost: "2.3140",
      actions_this_hour: 18,
      model_usage: [
        {
          model: "gpt-5.4",
          input_tokens: 28_450,
          output_tokens: 4_208,
          cost: "1.201400",
        },
        {
          model: "gpt-5.4-mini",
          input_tokens: 12_004,
          output_tokens: 7_102,
          cost: "1.112600",
        },
      ],
    };
  }

  getFeatureFlags(): FeatureFlagsResponse {
    return {
      ...this.featureFlags,
      fetched_at: nowIso(),
    };
  }

  subscribeToChat(subscriber: EventSubscriber<ChatSseEvent>): () => void {
    this.chatSubscribers.add(subscriber);
    return () => {
      this.chatSubscribers.delete(subscriber);
    };
  }

  subscribeToLogs(subscriber: EventSubscriber<LogEntry>): () => void {
    this.logSubscribers.add(subscriber);
    for (const entry of this.logs.slice(-25)) {
      subscriber.send(entry);
    }
    return () => {
      this.logSubscribers.delete(subscriber);
    };
  }

  private publishChatEvent(event: ChatSseEvent): void {
    for (const subscriber of this.chatSubscribers) {
      subscriber.send(event);
    }
  }

  private pushLog(
    message: string,
    source: string,
    level: LogEntry["level"] = "info"
  ): LogEntry {
    const entry: LogEntry = {
      id: this.nextId("log"),
      level,
      timestamp: nowIso(),
      message,
      source,
    };
    this.logs.unshift(entry);
    while (this.logs.length > 100) {
      this.logs.pop();
    }
    for (const subscriber of this.logSubscribers) {
      subscriber.send(entry);
    }
    return entry;
  }

  listThreads(): ThreadListResponse {
    const assistant_thread = this.threads.get("thread-assistant")?.info ?? null;
    const threads = [...this.threads.values()]
      .filter((thread) => thread.info.id !== "thread-assistant")
      .sort((left, right) =>
        right.info.updated_at.localeCompare(left.info.updated_at)
      )
      .map((thread) => thread.info);

    return {
      assistant_thread,
      threads,
      active_thread: this.activeThreadId,
    };
  }

  createThread(): ThreadInfo {
    const id = this.nextId("thread");
    const threadInfo: ThreadInfo = {
      id,
      state: "Idle",
      turn_count: 0,
      created_at: nowIso(),
      updated_at: nowIso(),
      title: "New planning thread",
      thread_type: "thread",
      channel: "gateway",
    };
    this.threads.set(id, {
      info: threadInfo,
      turns: [],
    });
    this.activeThreadId = id;
    this.pushLog(`Created thread ${id}.`, "chat");
    return threadInfo;
  }

  getHistory(threadId?: string | null): HistoryResponse {
    const fallbackThreadId =
      threadId ?? this.activeThreadId ?? this.threads.keys().next().value;
    const thread = this.threads.get(fallbackThreadId);
    if (!thread) {
      throw new Error("Thread not found");
    }
    this.activeThreadId = thread.info.id;
    return {
      thread_id: thread.info.id,
      turns: thread.turns,
      has_more: false,
      oldest_timestamp: thread.turns[0]?.started_at,
      pending_approval: thread.pendingApproval,
    };
  }

  sendMessage(request: SendMessageRequest): SendMessageResponse {
    const requestedThreadId =
      request.thread_id && this.threads.has(request.thread_id)
        ? request.thread_id
        : this.activeThreadId;
    const targetThread = this.threads.get(requestedThreadId);
    if (!targetThread) {
      throw new Error("Thread not found");
    }

    const started_at = nowIso();
    const turn: TurnInfo = {
      turn_number: targetThread.turns.length + 1,
      user_input: request.content,
      response: null,
      state: "Processing",
      started_at,
      completed_at: null,
      tool_calls: [],
    };

    targetThread.turns = [...targetThread.turns, turn];
    targetThread.info = {
      ...targetThread.info,
      turn_count: targetThread.turns.length,
      updated_at: started_at,
      state: "Processing",
    };
    this.threads.set(targetThread.info.id, targetThread);
    this.activeThreadId = targetThread.info.id;

    const message_id = this.nextId("message");
    this.pushLog(
      `Accepted chat turn ${message_id} for ${targetThread.info.id}.`,
      "chat"
    );

    this.publishChatEvent({
      type: "thinking",
      message: "Planning the next assistant response.",
      thread_id: targetThread.info.id,
    });

    const toolName =
      request.content.toLowerCase().includes("log") ||
      request.content.toLowerCase().includes("inspect")
        ? "inspect_preview_stack"
        : "write_preview_summary";

    setTimeout(() => {
      turn.tool_calls = [
        {
          name: toolName,
          has_result: false,
          has_error: false,
        },
      ];
      this.publishChatEvent({
        type: "tool_started",
        name: toolName,
        thread_id: targetThread.info.id,
      });
    }, 120);

    setTimeout(() => {
      this.publishChatEvent({
        type: "tool_result",
        name: toolName,
        preview:
          "Collected current route state, mock transport health, and feature-flag visibility.",
        thread_id: targetThread.info.id,
      });
      turn.tool_calls = [
        {
          name: toolName,
          has_result: true,
          has_error: false,
          result_preview:
            "Collected current route state, mock transport health, and feature-flag visibility.",
        },
      ];
    }, 260);

    setTimeout(() => {
      this.publishChatEvent({
        type: "tool_completed",
        name: toolName,
        success: true,
        thread_id: targetThread.info.id,
      });
    }, 340);

    const fullResponse = `Mock backend response for "${request.content}": the preview is now wired to typed JSON and SSE routes instead of local screen fixture arrays.`;
    const chunks = [
      "Mock backend response for ",
      `"${request.content}": `,
      "the preview is now wired to typed JSON and SSE routes ",
      "instead of local screen fixture arrays.",
    ];

    chunks.forEach((content, index) => {
      setTimeout(() => {
        this.publishChatEvent({
          type: "stream_chunk",
          content,
          thread_id: targetThread.info.id,
        });
      }, 420 + index * 70);
    });

    setTimeout(() => {
      turn.response = fullResponse;
      turn.state = "Completed";
      turn.completed_at = nowIso();
      targetThread.info = {
        ...targetThread.info,
        state: "Idle",
        turn_count: targetThread.turns.length,
        updated_at: turn.completed_at,
      };
      this.publishChatEvent({
        type: "response",
        content: fullResponse,
        thread_id: targetThread.info.id,
      });
      this.pushLog(
        `Completed streamed response for ${targetThread.info.id}.`,
        "chat"
      );
    }, 820);

    return {
      message_id,
      status: "accepted",
    };
  }

  submitApproval(request: ApprovalRequest): ActionResponse {
    const thread = [...this.threads.values()].find(
      (candidate) => candidate.pendingApproval?.request_id === request.request_id
    );
    if (!thread || !thread.pendingApproval) {
      throw new Error("Pending approval not found");
    }

    const action =
      request.action === "always"
        ? "always approve"
        : request.action === "deny"
          ? "deny"
          : "approve";

    const pending = thread.pendingApproval;
    thread.pendingApproval = undefined;
    thread.info = {
      ...thread.info,
      state: "Idle",
      updated_at: nowIso(),
    };

    const latestTurn = thread.turns.at(-1);
    if (latestTurn) {
      latestTurn.state = request.action === "deny" ? "Failed" : "Completed";
      latestTurn.completed_at = nowIso();
      if (request.action === "deny") {
        latestTurn.response =
          "The pending action was denied, so no file changes were applied.";
      } else {
        latestTurn.response =
          "Approval received. Proceeding with the approved mock backend implementation slice.";
      }
    }

    this.publishChatEvent({
      type: request.action === "deny" ? "error" : "status",
      message:
        request.action === "deny"
          ? `Denied ${pending.tool_name}.`
          : `${action} confirmed for ${pending.tool_name}.`,
      thread_id: thread.info.id,
    });

    this.pushLog(
      `${action} recorded for ${pending.tool_name} in ${thread.info.id}.`,
      "chat",
      request.action === "deny" ? "warn" : "info"
    );

    return createActionResponse(
      request.action === "deny"
        ? "Request denied."
        : "Approval recorded and the conversation resumed."
    );
  }

  getMemoryTree(depth?: number): MemoryTreeResponse {
    const seenDirectories = new Set<string>();
    const entries = [...this.memoryDocuments.keys()]
      .flatMap((path) => {
        const parts = path.split("/");
        const results: { path: string; is_dir: boolean }[] = [];
        for (let index = 0; index < parts.length - 1; index += 1) {
          const dirPath = parts.slice(0, index + 1).join("/");
          if (!seenDirectories.has(dirPath)) {
            seenDirectories.add(dirPath);
            results.push({ path: dirPath, is_dir: true });
          }
        }
        results.push({ path, is_dir: false });
        return results;
      })
      .filter((entry) =>
        typeof depth === "number"
          ? entry.path.split("/").length <= depth
          : true
      )
      .sort((left, right) => left.path.localeCompare(right.path));

    return { entries };
  }

  listMemory(path = ""): MemoryListResponse {
    const prefix = path.length > 0 ? `${path}/` : "";
    const names = new Map<
      string,
      { name: string; path: string; is_dir: boolean; updated_at: string | null }
    >();

    for (const [documentPath, document] of this.memoryDocuments.entries()) {
      if (!documentPath.startsWith(prefix)) {
        continue;
      }
      const remainder = documentPath.slice(prefix.length);
      if (remainder.length === 0) {
        continue;
      }
      const [segment, ...rest] = remainder.split("/");
      const entryPath = path.length > 0 ? `${path}/${segment}` : segment;
      if (rest.length > 0) {
        if (!names.has(entryPath)) {
          names.set(entryPath, {
            name: segment,
            path: entryPath,
            is_dir: true,
            updated_at: null,
          });
        }
      } else {
        names.set(entryPath, {
          name: segment,
          path: entryPath,
          is_dir: false,
          updated_at: document.updated_at,
        });
      }
    }

    return {
      path,
      entries: [...names.values()].sort((left, right) =>
        left.path.localeCompare(right.path)
      ),
    };
  }

  readMemory(path: string): MemoryReadResponse {
    const document = this.memoryDocuments.get(path);
    if (!document) {
      throw new Error("Document not found");
    }
    return {
      path,
      content: document.content,
      updated_at: document.updated_at,
    };
  }

  writeMemory(request: MemoryWriteRequest): MemoryWriteResponse {
    this.memoryDocuments.set(request.path, {
      content: request.content,
      updated_at: nowIso(),
    });
    this.pushLog(`Memory document updated: ${request.path}.`, "memory");
    return {
      path: request.path,
      status: "written",
    };
  }

  searchMemory(request: MemorySearchRequest): MemorySearchResponse {
    const query = request.query.trim().toLowerCase();
    if (query.length === 0) {
      return { results: [] };
    }

    const results: SearchHit[] = [];
    for (const [path, document] of this.memoryDocuments.entries()) {
      const haystack = `${path}\n${document.content}`.toLowerCase();
      const score = haystack.includes(query)
        ? 1 / Math.max(1, haystack.indexOf(query) + 1)
        : 0;
      if (score > 0) {
        results.push({
          path,
          content: document.content.slice(0, 180),
          score,
        });
      }
    }

    return {
      results: results
        .sort((left, right) => right.score - left.score)
        .slice(0, request.limit ?? 10),
    };
  }

  listJobs(): JobListResponse {
    return {
      jobs: [...this.jobs.values()].map(({ detail }) => ({
        id: detail.id,
        title: detail.title,
        state: detail.state,
        user_id: detail.user_id,
        created_at: detail.created_at,
        started_at: detail.started_at,
      })),
    };
  }

  summarizeJobs(): JobSummaryResponse {
    const jobs = [...this.jobs.values()].map((job) => job.detail);
    return {
      total: jobs.length,
      pending: jobs.filter((job) => job.state === "pending").length,
      in_progress: jobs.filter((job) => job.state === "in_progress").length,
      completed: jobs.filter((job) => job.state === "completed").length,
      failed: jobs.filter((job) => job.state === "failed").length,
      stuck: jobs.filter((job) => job.state === "stuck").length,
    };
  }

  getJob(id: string): JobDetailResponse {
    const job = this.jobs.get(id);
    if (!job) {
      throw new Error("Job not found");
    }
    return job.detail;
  }

  getJobEvents(id: string): JobEventsResponse {
    const job = this.jobs.get(id);
    if (!job) {
      throw new Error("Job not found");
    }
    return {
      events: job.events,
    };
  }

  listJobFiles(id: string): ProjectFilesResponse {
    const job = this.jobs.get(id);
    if (!job) {
      throw new Error("Job not found");
    }
    return {
      entries: Object.keys(job.files).map((path) => ({
        name: path.split("/").at(-1) ?? path,
        path,
        is_dir: false,
      })),
    };
  }

  readJobFile(id: string, path: string): ProjectFileReadResponse {
    const job = this.jobs.get(id);
    if (!job) {
      throw new Error("Job not found");
    }
    const content = job.files[path];
    if (typeof content !== "string") {
      throw new Error("File not found");
    }
    return { path, content };
  }

  restartJob(id: string): ActionResponse {
    const job = this.jobs.get(id);
    if (!job) {
      throw new Error("Job not found");
    }
    const from = job.detail.state;
    job.detail = {
      ...job.detail,
      state: "in_progress",
      started_at: nowIso(),
      completed_at: null,
      elapsed_secs: 0,
      can_restart: false,
      can_prompt: true,
      transitions: [
        ...job.detail.transitions,
        {
          from,
          to: "in_progress",
          timestamp: nowIso(),
          reason: "Restarted from the mock preview UI.",
        },
      ],
    };
    job.events = [
      {
        id: this.nextId("job-event"),
        level: "info",
        message: "Job restarted from the preview detail panel.",
        timestamp: nowIso(),
      },
      ...job.events,
    ];
    this.pushLog(`Restarted job ${id}.`, "jobs");
    return createActionResponse("Job restarted.");
  }

  cancelJob(id: string): ActionResponse {
    const job = this.jobs.get(id);
    if (!job) {
      throw new Error("Job not found");
    }
    const from = job.detail.state;
    job.detail = {
      ...job.detail,
      state: "failed",
      completed_at: nowIso(),
      can_restart: true,
      can_prompt: false,
      transitions: [
        ...job.detail.transitions,
        {
          from,
          to: "failed",
          timestamp: nowIso(),
          reason: "Cancelled from the mock preview UI.",
        },
      ],
    };
    job.events = [
      {
        id: this.nextId("job-event"),
        level: "warn",
        message: "Job cancelled from the preview detail panel.",
        timestamp: nowIso(),
      },
      ...job.events,
    ];
    this.pushLog(`Cancelled job ${id}.`, "jobs", "warn");
    return createActionResponse("Job cancelled.");
  }

  promptJob(id: string, request: JobPromptRequest): ActionResponse {
    const job = this.jobs.get(id);
    if (!job) {
      throw new Error("Job not found");
    }
    job.events = [
      {
        id: this.nextId("job-event"),
        level: "info",
        message: `Follow-up prompt submitted: ${request.prompt}`,
        timestamp: nowIso(),
      },
      ...job.events,
    ];
    this.pushLog(`Submitted follow-up prompt for ${id}.`, "jobs");
    return createActionResponse("Prompt submitted to the job.");
  }

  listRoutines(): RoutineListResponse {
    return {
      routines: [...this.routines.values()].map(({ detail }) => ({
        id: detail.id,
        name: detail.name,
        description: detail.description,
        enabled: detail.enabled,
        trigger_type: String(detail.trigger.type ?? "manual"),
        trigger_summary:
          typeof detail.trigger.schedule === "string"
            ? `cron: ${detail.trigger.schedule}`
            : typeof detail.trigger.pattern === "string"
              ? `on ${detail.trigger.pattern}`
              : "manual only",
        action_type: String(detail.action.type ?? "lightweight"),
        last_run_at: detail.last_run_at,
        next_fire_at: detail.next_fire_at,
        run_count: detail.run_count,
        consecutive_failures: detail.consecutive_failures,
        status: statusFromRoutine(detail),
      })),
    };
  }

  summarizeRoutines(): RoutineSummaryResponse {
    const routines = [...this.routines.values()].map((routine) => routine.detail);
    const total = routines.length;
    const enabled = routines.filter((routine) => routine.enabled).length;
    const disabled = total - enabled;
    const failing = routines.filter(
      (routine) => routine.consecutive_failures > 0
    ).length;
    const todayPrefix = nowIso().slice(0, 10);
    const runs_today = routines.filter((routine) =>
      routine.last_run_at?.startsWith(todayPrefix)
    ).length;
    return {
      total,
      enabled,
      disabled,
      failing,
      runs_today,
    };
  }

  getRoutine(id: string): RoutineDetailResponse {
    const routine = this.routines.get(id);
    if (!routine) {
      throw new Error("Routine not found");
    }
    return routine.detail;
  }

  getRoutineRuns(id: string): RoutineRunsResponse {
    const routine = this.routines.get(id);
    if (!routine) {
      throw new Error("Routine not found");
    }
    return {
      runs: routine.detail.recent_runs,
    };
  }

  triggerRoutine(id: string): ActionResponse {
    const routine = this.routines.get(id);
    if (!routine) {
      throw new Error("Routine not found");
    }
    const run = {
      id: this.nextId("routine-run"),
      trigger_type: "manual",
      started_at: nowIso(),
      completed_at: nowIso(),
      status: "completed",
      result_summary: "Manual trigger completed in the preview shell.",
      tokens_used: 142,
      job_id: null,
    };
    routine.detail = {
      ...routine.detail,
      last_run_at: run.started_at,
      run_count: routine.detail.run_count + 1,
      recent_runs: [run, ...routine.detail.recent_runs].slice(0, 10),
    };
    this.pushLog(`Triggered routine ${id}.`, "routines");
    return createActionResponse("Routine triggered.");
  }

  toggleRoutine(id: string, request?: ToggleRequest): ActionResponse {
    const routine = this.routines.get(id);
    if (!routine) {
      throw new Error("Routine not found");
    }
    const enabled = request?.enabled ?? !routine.detail.enabled;
    routine.detail = {
      ...routine.detail,
      enabled,
    };
    this.pushLog(
      `${enabled ? "Enabled" : "Disabled"} routine ${id}.`,
      "routines",
      enabled ? "info" : "warn"
    );
    return createActionResponse(enabled ? "Routine enabled." : "Routine disabled.");
  }

  deleteRoutine(id: string): ActionResponse {
    if (!this.routines.has(id)) {
      throw new Error("Routine not found");
    }
    this.routines.delete(id);
    this.pushLog(`Deleted routine ${id}.`, "routines", "warn");
    return createActionResponse("Routine deleted.");
  }

  listExtensions(): { extensions: ExtensionInfo[] } {
    return {
      extensions: [...this.extensions.values()].map((extension) => extension.info),
    };
  }

  listExtensionTools(): ToolListResponse {
    const tools: ToolInfo[] = [
      {
        name: "create_job",
        description: "Create a preview job from the current screen context.",
      },
      {
        name: "extension_info",
        description: "Inspect the registered extension metadata.",
      },
      ...[...this.extensions.values()].flatMap((extension) =>
        extension.info.tools.map((toolName) => ({
          name: toolName,
          description: `${extension.info.display_name ?? extension.info.name} tool`,
        }))
      ),
    ];
    return { tools };
  }

  searchExtensionRegistry(query?: string | null): RegistrySearchResponse {
    const needle = query?.trim().toLowerCase() ?? "";
    return {
      entries: [...this.registryEntries.values()].filter((entry) => {
        if (needle.length === 0) {
          return true;
        }
        return (
          entry.name.toLowerCase().includes(needle) ||
          entry.display_name.toLowerCase().includes(needle) ||
          entry.description.toLowerCase().includes(needle) ||
          entry.keywords.some((keyword) => keyword.toLowerCase().includes(needle))
        );
      }),
    };
  }

  installExtension(name: string): ActionResponse {
    const registryEntry = this.registryEntries.get(name);
    if (!registryEntry) {
      throw new Error("Registry entry not found");
    }
    registryEntry.installed = true;
    if (!this.extensions.has(name)) {
      this.extensions.set(name, {
        info: {
          name: registryEntry.name,
          display_name: registryEntry.display_name,
          kind: registryEntry.kind,
          description: registryEntry.description,
          authenticated: false,
          active: false,
          tools: [],
          needs_setup: true,
          has_auth: registryEntry.name === "slack",
          activation_status: "installed",
          version: registryEntry.version,
        },
        setupSecrets: [
          {
            name: "token",
            prompt: `${registryEntry.display_name} token`,
            optional: false,
            provided: false,
            auto_generate: false,
          },
        ],
      });
    }
    this.pushLog(`Installed extension ${name}.`, "extensions");
    return createActionResponse(
      `${registryEntry.display_name} installed into the mock preview.`
    );
  }

  activateExtension(name: string): ActionResponse {
    const extension = this.extensions.get(name);
    if (!extension) {
      throw new Error("Extension not found");
    }
    if (extension.info.has_auth && !extension.info.authenticated) {
      this.publishChatEvent({
        type: "auth_required",
        extension_name: name,
        instructions:
          "Provide a token in the setup panel to complete activation.",
        setup_url: `/api/extensions/${name}/setup`,
      });
      this.pushLog(
        `Activation for ${name} is waiting for a manual token.`,
        "extensions",
        "warn"
      );
      return createActionResponse("Manual token required before activation.", {
        awaiting_token: true,
        instructions:
          "Provide a token in the setup panel to complete activation.",
        activated: false,
      });
    }
    extension.info = {
      ...extension.info,
      active: true,
      activation_status: "active",
    };
    this.publishChatEvent({
      type: "extension_status",
      extension_name: name,
      status: "active",
      message: `${extension.info.display_name ?? name} is now active.`,
    });
    this.pushLog(`Activated extension ${name}.`, "extensions");
    return createActionResponse("Extension activated.", { activated: true });
  }

  removeExtension(name: string): ActionResponse {
    if (!this.extensions.has(name)) {
      throw new Error("Extension not found");
    }
    this.extensions.delete(name);
    const registryEntry = this.registryEntries.get(name);
    if (registryEntry) {
      registryEntry.installed = false;
    }
    this.pushLog(`Removed extension ${name}.`, "extensions", "warn");
    return createActionResponse("Extension removed.");
  }

  getExtensionSetup(name: string): ExtensionSetupResponse {
    const extension = this.extensions.get(name);
    if (!extension) {
      throw new Error("Extension not found");
    }
    return {
      name: extension.info.name,
      kind: extension.info.kind,
      secrets: extension.setupSecrets,
    };
  }

  submitExtensionSetup(
    name: string,
    secrets: Record<string, string>
  ): ActionResponse {
    const extension = this.extensions.get(name);
    if (!extension) {
      throw new Error("Extension not found");
    }
    extension.setupSecrets = extension.setupSecrets.map((field) => ({
      ...field,
      provided:
        field.provided ||
        (typeof secrets[field.name] === "string" &&
          secrets[field.name].trim().length > 0),
    }));
    extension.info = {
      ...extension.info,
      authenticated: extension.info.has_auth ? true : extension.info.authenticated,
      active: true,
      activation_status: "active",
    };
    this.publishChatEvent({
      type: "auth_completed",
      extension_name: name,
      success: true,
      message: `${extension.info.display_name ?? name} is ready to use.`,
    });
    this.pushLog(`Stored setup values for ${name}.`, "extensions");
    return createActionResponse("Extension setup saved.", { activated: true });
  }

  listSkills(): SkillListResponse {
    const skills = [...this.skills.values()];
    return {
      skills,
      count: skills.length,
    };
  }

  searchSkills(request: SkillSearchRequest): SkillSearchResponse {
    const query = request.query.trim().toLowerCase();
    const installed = [...this.skills.values()].filter((skill) => {
      if (query.length === 0) {
        return true;
      }
      return (
        skill.name.toLowerCase().includes(query) ||
        skill.description.toLowerCase().includes(query) ||
        skill.keywords.some((keyword) => keyword.toLowerCase().includes(query))
      );
    });
    const catalog = [...this.catalogSkills.values()].filter((entry) => {
      if (query.length === 0) {
        return true;
      }
      return (
        entry.name.toLowerCase().includes(query) ||
        entry.description.toLowerCase().includes(query) ||
        entry.keywords.some((keyword) => keyword.toLowerCase().includes(query))
      );
    });
    return {
      catalog,
      installed,
      registry_url: "https://clawhub.example.invalid",
    };
  }

  installSkill(request: SkillInstallRequest): ActionResponse {
    const catalogMatch =
      [...this.catalogSkills.values()].find(
        (entry) =>
          entry.name === request.name ||
          entry.slug === request.slug ||
          entry.slug.endsWith(`/${request.name}`)
      ) ?? null;
    const name = catalogMatch?.name ?? request.name;
    this.skills.set(name, {
      name,
      description:
        catalogMatch?.description ??
        "Installed ad hoc into the mock preview for UI validation.",
      version: catalogMatch?.version ?? "1.0.0",
      trust: "preview",
      source: request.url ? "url" : request.content ? "content" : "catalog",
      keywords: catalogMatch?.keywords ?? ["mock", "preview"],
    });
    this.pushLog(`Installed skill ${name}.`, "skills");
    return createActionResponse(`Skill ${name} installed.`);
  }

  removeSkill(name: string): ActionResponse {
    if (!this.skills.has(name)) {
      throw new Error("Skill not found");
    }
    this.skills.delete(name);
    this.pushLog(`Removed skill ${name}.`, "skills", "warn");
    return createActionResponse(`Skill ${name} removed.`);
  }

  getLogLevel(): LogLevelResponse {
    return {
      level: this.logLevel,
    };
  }

  setLogLevel(level: string): LogLevelResponse {
    this.logLevel = level;
    this.pushLog(`Log level changed to ${level}.`, "logs");
    return {
      level,
    };
  }
}
