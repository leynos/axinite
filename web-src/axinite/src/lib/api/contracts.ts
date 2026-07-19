export type Identifier = string;

export type GatewayStatusResponse = {
  version: string;
  sse_connections: number;
  ws_connections: number;
  total_connections: number;
  uptime_secs: number;
  restart_enabled: boolean;
  daily_cost?: string;
  actions_this_hour?: number;
  model_usage?: ModelUsageEntry[];
};

export type ModelUsageEntry = {
  model: string;
  input_tokens: number;
  output_tokens: number;
  cost: string;
};

export type FeatureFlagsResponse = {
  deployment: string;
  fetched_at: string;
  flags: Record<string, boolean>;
};

export type GatewayStatus = {
  label: string;
  detail: string;
};

export type ImageData = {
  media_type: string;
  data: string;
};

export type SendMessageRequest = {
  content: string;
  thread_id?: string | null;
  timezone?: string | null;
  images: ImageData[];
};

export type SendMessageResponse = {
  message_id: Identifier;
  status: string;
};

export type ThreadInfo = {
  id: Identifier;
  state: string;
  turn_count: number;
  created_at: string;
  updated_at: string;
  title?: string;
  thread_type?: string;
  channel?: string;
};

export type ThreadListResponse = {
  assistant_thread: ThreadInfo | null;
  threads: ThreadInfo[];
  active_thread: Identifier | null;
};

export type ToolCallInfo = {
  name: string;
  has_result: boolean;
  has_error: boolean;
  result_preview?: string;
  error?: string;
};

export type TurnInfo = {
  turn_number: number;
  user_input: string;
  response: string | null;
  state: string;
  started_at: string;
  completed_at: string | null;
  tool_calls: ToolCallInfo[];
};

export type PendingApprovalInfo = {
  request_id: string;
  tool_name: string;
  description: string;
  parameters: string;
};

export type HistoryResponse = {
  thread_id: Identifier;
  turns: TurnInfo[];
  has_more: boolean;
  oldest_timestamp?: string;
  pending_approval?: PendingApprovalInfo;
};

export type ApprovalRequest = {
  request_id: string;
  action: string;
  thread_id?: string | null;
};

export type ChatSseEvent =
  | {
      type: "response";
      content: string;
      thread_id: string;
    }
  | {
      type: "thinking";
      message: string;
      thread_id?: string;
    }
  | {
      type: "tool_started";
      name: string;
      thread_id?: string;
    }
  | {
      type: "tool_completed";
      name: string;
      success: boolean;
      error?: string;
      parameters?: string;
      thread_id?: string;
    }
  | {
      type: "tool_result";
      name: string;
      preview: string;
      thread_id?: string;
    }
  | {
      type: "stream_chunk";
      content: string;
      thread_id?: string;
    }
  | {
      type: "status";
      message: string;
      thread_id?: string;
    }
  | {
      type: "approval_needed";
      request_id: string;
      tool_name: string;
      description: string;
      parameters: string;
      thread_id?: string;
    }
  | {
      type: "auth_required";
      extension_name: string;
      instructions?: string;
      auth_url?: string;
      setup_url?: string;
    }
  | {
      type: "auth_completed";
      extension_name: string;
      success: boolean;
      message: string;
    }
  | {
      type: "error";
      message: string;
      thread_id?: string;
    }
  | {
      type: "heartbeat";
    }
  | {
      type: "extension_status";
      extension_name: string;
      status: string;
      message?: string;
    };

export type MemoryTreeResponse = {
  entries: TreeEntry[];
};

export type TreeEntry = {
  path: string;
  is_dir: boolean;
};

export type MemoryListResponse = {
  path: string;
  entries: ListEntry[];
};

export type ListEntry = {
  name: string;
  path: string;
  is_dir: boolean;
  updated_at: string | null;
};

export type MemoryReadResponse = {
  path: string;
  content: string;
  updated_at: string | null;
};

export type MemoryWriteRequest = {
  path: string;
  content: string;
};

export type MemoryWriteResponse = {
  path: string;
  status: string;
};

export type MemorySearchRequest = {
  query: string;
  limit?: number;
};

export type MemorySearchResponse = {
  results: SearchHit[];
};

export type SearchHit = {
  path: string;
  content: string;
  score: number;
};

export type JobInfo = {
  id: Identifier;
  title: string;
  state: string;
  user_id: string;
  created_at: string;
  started_at: string | null;
};

export type JobListResponse = {
  jobs: JobInfo[];
};

export type JobSummaryResponse = {
  total: number;
  pending: number;
  in_progress: number;
  completed: number;
  failed: number;
  stuck: number;
};

export type TransitionInfo = {
  from: string;
  to: string;
  timestamp: string;
  reason: string | null;
};

export type JobDetailResponse = {
  id: Identifier;
  title: string;
  description: string;
  state: string;
  user_id: string;
  created_at: string;
  started_at: string | null;
  completed_at: string | null;
  elapsed_secs: number | null;
  project_dir?: string;
  browse_url?: string;
  job_mode?: string;
  transitions: TransitionInfo[];
  can_restart: boolean;
  can_prompt: boolean;
  job_kind?: string;
};

export type JobEventInfo = {
  id: Identifier;
  level: "info" | "warn" | "error";
  message: string;
  timestamp: string;
};

export type JobEventsResponse = {
  events: JobEventInfo[];
};

export type ProjectFileEntry = {
  name: string;
  path: string;
  is_dir: boolean;
};

export type ProjectFilesResponse = {
  entries: ProjectFileEntry[];
};

export type ProjectFileReadResponse = {
  path: string;
  content: string;
};

export type JobPromptRequest = {
  prompt: string;
};

export type ActionResponse = {
  success: boolean;
  message: string;
  auth_url?: string;
  awaiting_token?: boolean;
  instructions?: string;
  activated?: boolean;
};

export type RoutineInfo = {
  id: Identifier;
  name: string;
  description: string;
  enabled: boolean;
  trigger_type: string;
  trigger_summary: string;
  action_type: string;
  last_run_at: string | null;
  next_fire_at: string | null;
  run_count: number;
  consecutive_failures: number;
  status: string;
};

export type RoutineListResponse = {
  routines: RoutineInfo[];
};

export type RoutineSummaryResponse = {
  total: number;
  enabled: number;
  disabled: number;
  failing: number;
  runs_today: number;
};

export type RoutineRunInfo = {
  id: Identifier;
  trigger_type: string;
  started_at: string;
  completed_at: string | null;
  status: string;
  result_summary: string | null;
  tokens_used: number | null;
  job_id: Identifier | null;
};

export type RoutineDetailResponse = {
  id: Identifier;
  name: string;
  description: string;
  enabled: boolean;
  trigger: Record<string, unknown>;
  action: Record<string, unknown>;
  guardrails: Record<string, unknown>;
  notify: Record<string, unknown>;
  last_run_at: string | null;
  next_fire_at: string | null;
  run_count: number;
  consecutive_failures: number;
  created_at: string;
  recent_runs: RoutineRunInfo[];
};

export type RoutineRunsResponse = {
  runs: RoutineRunInfo[];
};

export type ToggleRequest = {
  enabled?: boolean;
};

export type ExtensionInfo = {
  name: string;
  display_name?: string;
  kind: string;
  description?: string;
  url?: string;
  authenticated: boolean;
  active: boolean;
  tools: string[];
  needs_setup: boolean;
  has_auth: boolean;
  activation_status?: string;
  activation_error?: string;
  version?: string;
};

export type ExtensionListResponse = {
  extensions: ExtensionInfo[];
};

export type ToolInfo = {
  name: string;
  description: string;
};

export type ToolListResponse = {
  tools: ToolInfo[];
};

export type InstallExtensionRequest = {
  name: string;
  url?: string | null;
  kind?: string | null;
};

export type ExtensionSetupResponse = {
  name: string;
  kind: string;
  secrets: SecretFieldInfo[];
};

export type SecretFieldInfo = {
  name: string;
  prompt: string;
  optional: boolean;
  provided: boolean;
  auto_generate: boolean;
};

export type ExtensionSetupRequest = {
  secrets: Record<string, string>;
};

export type RegistryEntryInfo = {
  name: string;
  display_name: string;
  kind: string;
  description: string;
  keywords: string[];
  installed: boolean;
  version?: string;
};

export type RegistrySearchResponse = {
  entries: RegistryEntryInfo[];
};

export type SkillInfo = {
  name: string;
  description: string;
  version: string;
  trust: string;
  source: string;
  keywords: string[];
};

export type SkillListResponse = {
  skills: SkillInfo[];
  count: number;
};

export type SkillSearchRequest = {
  query: string;
};

export type CatalogSkillEntry = {
  slug: string;
  name: string;
  description: string;
  version: string;
  score: number;
  updatedAt: string;
  stars: number;
  downloads: number;
  owner: string;
  keywords: string[];
};

export type SkillSearchResponse = {
  catalog: CatalogSkillEntry[];
  installed: SkillInfo[];
  registry_url: string;
  catalog_error?: string;
};

export type SkillInstallRequest = {
  name: string;
  slug?: string | null;
  url?: string | null;
  content?: string | null;
};

export type LogEntry = {
  id: Identifier;
  level: "trace" | "debug" | "info" | "warn" | "error";
  timestamp: string;
  message: string;
  source: string;
};

export type LogLevelResponse = {
  level: string;
};
