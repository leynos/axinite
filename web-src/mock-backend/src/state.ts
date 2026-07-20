import type {
  ActionResponse,
  ApprovalRequest,
  AuthCancelRequest,
  AuthTokenRequest,
  CatalogueSkillEntry,
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
  PairingListResponse,
  PairingRequestInfo,
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
import type { MemoryDocument } from "./fixtures";
import {
  createSeedCatalogueSkills,
  createSeedExtensions,
  createSeedJobs,
  createSeedLogs,
  createSeedMemoryDocuments,
  createSeedPairingRequests,
  createSeedRegistryEntries,
  createSeedRoutines,
  createSeedSkills,
  createSeedThreads,
} from "./fixtures";
import {
  applyApprovalToLatestTurn,
  approvalActionLabel,
  buildApprovalOutcome,
  createActionResponse,
  emitChatTurnSequence,
  matchCatalogueSkill,
  nowIso,
  resolveSkillSource,
  statusFromRoutine,
} from "./state-helpers";

// A deterministic 1x1 transparent PNG used for the `image_generated`
// SSE fixture emitted when a chat prompt contains "image".
export const MOCK_GENERATED_IMAGE_DATA_URL =
  "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";

// Thrown by pairing approval when the deterministic rate-limit fixture code
// is submitted, so the transport layer can answer with a plain-text 429
// response matching the daemon's `PairingStoreError::ApproveRateLimited`
// shape (a bare `(StatusCode, String)` body, not JSON).
export class PairingRateLimitedError extends Error {}

type MemoryListingEntry = {
  name: string;
  path: string;
  is_dir: boolean;
  updated_at: string | null;
};

type EventSubscriber<T> = {
  send: (payload: T) => void;
  close: () => void;
};

export class MockBackendState {
  private readonly bootTime = Date.now();

  private nextCounter = 1;

  // Mirrors the compiled defaults in the gateway's
  // src/channels/web/handlers/features.rs and the registry in
  // axinite/src/lib/feature-flags/registry.ts.
  private readonly featureFlags: FeatureFlagsResponse = {
    route_chat: true,
    route_memory: true,
    route_jobs: true,
    route_routines: true,
    route_extensions: true,
    route_skills: true,
    route_logs: true,
    panel_logs: true,
    action_memory_edit: false,
    action_job_restart: false,
    action_routine_trigger: false,
    action_extension_install: false,
    action_skill_install: false,
    surface_tee_attestation: false,
  };

  private logLevel = "info";

  private readonly chatSubscribers = new Set<EventSubscriber<ChatSseEvent>>();

  private readonly logSubscribers = new Set<EventSubscriber<LogEntry>>();

  private readonly logs: LogEntry[] = createSeedLogs();

  private activeThreadId = "thread-review";

  private readonly threads = createSeedThreads();

  private readonly memoryDocuments = createSeedMemoryDocuments();

  private readonly jobs = createSeedJobs();

  private readonly routines = createSeedRoutines();

  private readonly extensions = createSeedExtensions();

  // Deterministic pending pairing request fixture for the "whatsapp" WASM
  // channel, so the extensions pairing stepper has an approvable row without
  // needing a real pairing handshake.
  private readonly pairingRequests = createSeedPairingRequests();

  private readonly registryEntries = createSeedRegistryEntries();

  private readonly skills = createSeedSkills();

  private readonly catalogueSkills = createSeedCatalogueSkills();

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

  // Flags resolve like the real gateway: a FEATURE_FLAG_<UPPER_SNAKE_NAME>
  // environment variable overrides the default (`true` enables, any other
  // set value disables), so stub runs can exercise flag combinations.
  getFeatureFlags(): FeatureFlagsResponse {
    const resolved: FeatureFlagsResponse = { ...this.featureFlags };
    for (const name of Object.keys(resolved)) {
      const raw = process.env[`FEATURE_FLAG_${name.toUpperCase()}`];
      if (typeof raw === "string") {
        resolved[name] = raw.toLowerCase() === "true";
      }
    }
    return resolved;
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
    target: string,
    level: LogEntry["level"] = "info"
  ): LogEntry {
    const entry: LogEntry = {
      level,
      timestamp: nowIso(),
      message,
      target,
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

    emitChatTurnSequence({
      request,
      thread: targetThread,
      turn,
      imageDataUrl: MOCK_GENERATED_IMAGE_DATA_URL,
      publish: (event) => this.publishChatEvent(event),
      pushLog: (message, target, level) => this.pushLog(message, target, level),
      now: nowIso,
    });

    return {
      message_id,
      status: "accepted",
    };
  }

  submitApproval(request: ApprovalRequest): ActionResponse {
    const thread = [...this.threads.values()].find(
      (candidate) => candidate.pendingApproval?.request_id === request.request_id
    );
    if (!thread?.pendingApproval) {
      throw new Error("Pending approval not found");
    }

    const isDeny = request.action === "deny";
    const action = approvalActionLabel(request.action);
    const pending = thread.pendingApproval;
    const outcome = buildApprovalOutcome({
      isDeny,
      action,
      toolName: pending.tool_name,
    });
    thread.pendingApproval = undefined;
    thread.info = {
      ...thread.info,
      state: "Idle",
      updated_at: nowIso(),
    };

    applyApprovalToLatestTurn({ thread, isDeny, now: nowIso });

    this.publishChatEvent({
      type: outcome.eventType,
      message: outcome.message,
      thread_id: thread.info.id,
    });

    this.pushLog(
      `${action} recorded for ${pending.tool_name} in ${thread.info.id}.`,
      "chat",
      outcome.logLevel
    );

    return createActionResponse(outcome.response);
  }

  // Submit an extension auth token, bypassing the message pipeline, mirroring
  // POST /api/chat/auth-token. Deterministic acceptance rule: the literal
  // token "valid-token", or any token of length >= 8, succeeds; anything
  // shorter fails.
  chatAuthToken(request: AuthTokenRequest): ActionResponse {
    const accepted =
      request.token === "valid-token" || request.token.length >= 8;
    if (accepted) {
      const message = "Authentication completed.";
      this.publishChatEvent({
        type: "auth_completed",
        extension_name: request.extension_name,
        success: true,
        message,
      });
      this.pushLog(
        `Authenticated ${request.extension_name} via chat auth-token.`,
        "extensions"
      );
      return createActionResponse(message, { activated: true });
    }
    this.pushLog(
      `Rejected auth token for ${request.extension_name}.`,
      "extensions",
      "warn"
    );
    return createActionResponse(
      "Invalid or missing authentication token.",
      { success: false }
    );
  }

  chatAuthCancel(request: AuthCancelRequest): ActionResponse {
    this.pushLog(
      `Cancelled auth flow for ${request.extension_name}.`,
      "extensions",
      "warn"
    );
    return createActionResponse("Auth cancelled.");
  }

  // Mirrors GET /api/pairing/{channel}: unknown channels answer with an
  // empty pending-request list rather than a 404.
  pairingList(channel: string): PairingListResponse {
    return {
      channel,
      requests: this.pairingRequests.get(channel) ?? [],
    };
  }

  // Mirrors POST /api/pairing/{channel}/approve. Submitting the code
  // "rate-limited" throws PairingRateLimitedError so the transport layer can
  // answer with the daemon's plain-text 429 fixture.
  pairingApprove(channel: string, code: string): ActionResponse {
    if (code === "rate-limited") {
      throw new PairingRateLimitedError(
        "Too many failed approve attempts; try again later"
      );
    }
    const pending = this.pairingRequests.get(channel) ?? [];
    const index = pending.findIndex((request) => request.code === code);
    if (index === -1) {
      return createActionResponse("Invalid or expired pairing code", {
        success: false,
      });
    }
    const [approved] = pending.splice(index, 1);
    this.pairingRequests.set(channel, pending);
    this.pushLog(
      `Approved pairing request ${code} for ${channel}.`,
      "extensions"
    );
    return createActionResponse(
      `Pairing approved for sender '${approved.sender_id}'`
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

  // Fold a single stored document into the listing for `parentPath`. Returns
  // early when the document lies outside the requested directory; otherwise
  // records the immediate child as either a directory placeholder or a leaf
  // file. A leaf always overwrites a placeholder, but a placeholder never
  // displaces an already-recorded leaf.
  private addMemoryListingEntry(
    entries: Map<string, MemoryListingEntry>,
    parentPath: string,
    documentPath: string,
    document: MemoryDocument
  ): void {
    const prefix = parentPath.length > 0 ? `${parentPath}/` : "";
    if (!documentPath.startsWith(prefix)) {
      return;
    }

    const remainder = documentPath.slice(prefix.length);
    if (remainder.length === 0) {
      return;
    }

    const [segment, ...rest] = remainder.split("/");
    const entryPath = parentPath.length > 0 ? `${parentPath}/${segment}` : segment;
    const isDirectory = rest.length > 0;

    if (isDirectory) {
      if (!entries.has(entryPath)) {
        entries.set(entryPath, {
          name: segment,
          path: entryPath,
          is_dir: true,
          updated_at: null,
        });
      }
      return;
    }

    entries.set(entryPath, {
      name: segment,
      path: entryPath,
      is_dir: false,
      updated_at: document.updated_at,
    });
  }

  listMemory(path = ""): MemoryListResponse {
    const entries = new Map<string, MemoryListingEntry>();

    for (const [documentPath, document] of this.memoryDocuments.entries()) {
      this.addMemoryListingEntry(entries, path, documentPath, document);
    }

    return {
      path,
      entries: [...entries.values()].sort((left, right) =>
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
        message: `Follow-up prompt submitted: ${request.content}`,
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
      // The "google-drive" fixture exercises the OAuth auth-card dispatch
      // path (an `auth_url` to open in a new tab); every other has_auth
      // extension keeps the existing configure-modal (`setup_url` only)
      // path.
      const isOAuthFixture = name === "google-drive";
      const authUrl = isOAuthFixture
        ? "https://oauth.example.invalid/consent"
        : undefined;
      const setupUrl = isOAuthFixture
        ? "https://example.invalid/token-help"
        : `/api/extensions/${name}/setup`;
      const instructions = isOAuthFixture
        ? "Sign in with Google and grant Drive access, then return to finish activation."
        : "Provide a token in the setup panel to complete activation.";

      this.publishChatEvent({
        type: "auth_required",
        extension_name: name,
        instructions,
        auth_url: authUrl,
        setup_url: setupUrl,
      });
      this.pushLog(
        `Activation for ${name} is waiting for a manual token.`,
        "extensions",
        "warn"
      );
      return createActionResponse("Manual token required before activation.", {
        awaiting_token: true,
        instructions,
        activated: false,
        auth_url: authUrl,
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
    const catalogue = [...this.catalogueSkills.values()].filter((entry) => {
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
      catalog: catalogue,
      installed,
      registry_url: "https://clawhub.example.invalid",
    };
  }

  installSkill(request: SkillInstallRequest): ActionResponse {
    const catalogueMatch = matchCatalogueSkill(
      [...this.catalogueSkills.values()],
      request
    );
    const name = catalogueMatch?.name ?? request.name;
    this.skills.set(name, {
      name,
      description:
        catalogueMatch?.description ??
        "Installed ad hoc into the mock preview for UI validation.",
      version: catalogueMatch?.version ?? "1.0.0",
      trust: "preview",
      source: resolveSkillSource(request),
      keywords: catalogueMatch?.keywords ?? ["mock", "preview"],
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
