import type {
  CatalogueSkillEntry,
  ExtensionInfo,
  JobDetailResponse,
  JobEventInfo,
  LogEntry,
  PairingRequestInfo,
  PendingApprovalInfo,
  RegistryEntryInfo,
  RoutineDetailResponse,
  SecretFieldInfo,
  SkillInfo,
  ThreadInfo,
  TurnInfo,
} from "../../axinite/src/lib/api/contracts";

export type MemoryDocument = {
  content: string;
  updated_at: string;
};

export type MockThread = {
  info: ThreadInfo;
  turns: TurnInfo[];
  pendingApproval?: PendingApprovalInfo;
};

export type MockJob = {
  detail: JobDetailResponse;
  events: JobEventInfo[];
  files: Record<string, string>;
};

export type MockRoutine = {
  detail: RoutineDetailResponse;
};

export type MockExtension = {
  info: ExtensionInfo;
  setupSecrets: SecretFieldInfo[];
};

export type MockCatalogueSkill = CatalogueSkillEntry;

const DAY_MS = 24 * 60 * 60 * 1000;
const START_TIME = new Date("2026-03-26T11:30:00.000Z");

// Seed-fixture timestamps are expressed as "minutes before the deterministic
// START_TIME" so the mock preview renders a stable, reproducible history.
function iso(minutesAgo = 0): string {
  return new Date(START_TIME.getTime() - minutesAgo * 60 * 1000).toISOString();
}

// The seed data below is declared as module-level constants (not function
// bodies) so it reads as plain fixture data. Each `createSeed*` builder returns
// a fresh deep copy via `structuredClone`, giving every MockBackendState
// instance an isolated, mutable snapshot exactly as per-instance field literals
// did previously.

const SEED_LOGS: LogEntry[] = [
    {
      level: "info",
      message: "Mock gateway booted and attached to the static preview.",
      target: "gateway",
      timestamp: iso(35),
    },
    {
      level: "debug",
      message: "Runtime feature flags resolved for the mock preview shell.",
      target: "gateway",
      timestamp: iso(24),
    },
    {
      level: "info",
      message: "Locale bundles loaded successfully for en-GB.",
      target: "i18n",
      timestamp: iso(16),
    },
    {
      level: "info",
      message: "Initial route queries completed against the mock backend.",
      target: "frontend",
      timestamp: iso(8),
    },
];

export function createSeedLogs(): LogEntry[] {
  return structuredClone(SEED_LOGS);
}

const SEED_THREADS: [string, MockThread][] = [
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
];

export function createSeedThreads(): Map<string, MockThread> {
  return new Map(structuredClone(SEED_THREADS));
}

const SEED_MEMORY_DOCUMENTS: [string, MemoryDocument][] = [
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
];

export function createSeedMemoryDocuments(): Map<string, MemoryDocument> {
  return new Map(structuredClone(SEED_MEMORY_DOCUMENTS));
}

const SEED_JOBS: [string, MockJob][] = [
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
          project_dir: "/workspace/axinite",
          browse_url: "/jobs",
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
          project_dir: "/workspace/axinite",
          browse_url: "/jobs",
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
          project_dir: "/workspace/axinite",
          browse_url: "/extensions",
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
          project_dir: "/workspace/axinite",
          browse_url: "/skills",
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
          project_dir: "/workspace/axinite",
          browse_url: "/chat",
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
];

export function createSeedJobs(): Map<string, MockJob> {
  return new Map(structuredClone(SEED_JOBS));
}

const SEED_ROUTINES: [string, MockRoutine][] = [
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
          action: { type: "lightweight", prompt: "Summarize active preview work" },
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
          action: { type: "lightweight", prompt: "Summarize new warnings" },
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
];

export function createSeedRoutines(): Map<string, MockRoutine> {
  return new Map(structuredClone(SEED_ROUTINES));
}

const SEED_EXTENSIONS: [string, MockExtension][] = [
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
    [
      "whatsapp",
      {
        info: {
          name: "whatsapp",
          display_name: "WhatsApp",
          kind: "wasm_channel",
          description:
            "Pairing-gated messaging channel used to exercise the extensions pairing stepper.",
          url: "https://www.whatsapp.com",
          authenticated: false,
          active: false,
          tools: ["send_message"],
          needs_setup: true,
          has_auth: false,
          activation_status: "pairing",
          version: "0.1.0",
        },
        setupSecrets: [],
      },
    ],
    [
      "google-drive",
      {
        info: {
          name: "google-drive",
          display_name: "Google Drive",
          kind: "mcp_server",
          description:
            "OAuth-gated Drive file access used to exercise the auth-card dispatch path.",
          url: "https://drive.google.com",
          authenticated: false,
          active: false,
          tools: ["list_files", "read_file"],
          needs_setup: true,
          has_auth: true,
          activation_status: "configured",
          version: "0.1.0",
        },
        setupSecrets: [],
      },
    ],
];

export function createSeedExtensions(): Map<string, MockExtension> {
  return new Map(structuredClone(SEED_EXTENSIONS));
}

const SEED_PAIRING_REQUESTS: [string, PairingRequestInfo[]][] = [
    [
      "whatsapp",
      [
        {
          code: "PAIR-1234",
          sender_id: "user-42",
          created_at: iso(5),
        },
      ],
    ],
];

export function createSeedPairingRequests(): Map<string, PairingRequestInfo[]> {
  return new Map(structuredClone(SEED_PAIRING_REQUESTS));
}

const SEED_REGISTRY_ENTRIES: [string, RegistryEntryInfo][] = [
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
];

export function createSeedRegistryEntries(): Map<string, RegistryEntryInfo> {
  return new Map(structuredClone(SEED_REGISTRY_ENTRIES));
}

const SEED_SKILLS: [string, SkillInfo][] = [
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
];

export function createSeedSkills(): Map<string, SkillInfo> {
  return new Map(structuredClone(SEED_SKILLS));
}

const SEED_CATALOGUE_SKILLS: [string, MockCatalogueSkill][] = [
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
];

export function createSeedCatalogueSkills(): Map<string, MockCatalogueSkill> {
  return new Map(structuredClone(SEED_CATALOGUE_SKILLS));
}
