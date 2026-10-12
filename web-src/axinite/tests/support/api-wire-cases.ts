/**
 * @file HTTP wire examples anchored to the gateway handler routes and DTOs in
 * src/channels/web/handlers and src/channels/web/types. Identifier and query
 * examples deliberately contain separators so escaping is observable.
 */

import * as chat from "@/lib/api/chat";
import * as extensions from "@/lib/api/extensions";
import * as gateway from "@/lib/api/gateway";
import * as jobs from "@/lib/api/jobs";
import * as logs from "@/lib/api/logs";
import * as memory from "@/lib/api/memory";
import * as pairing from "@/lib/api/pairing";
import * as routines from "@/lib/api/routines";
import * as skills from "@/lib/api/skills";

export type WireCase = {
  name: string;
  call: () => Promise<unknown>;
  path: string;
  method?: string;
  body?: unknown;
  response?: unknown;
  result?: unknown;
  fallback?: unknown;
};

const id = "a/b ?#";
const segment = "a%2Fb%20%3F%23";
const query = "a%2Fb+%3F%23";
const send = {
  content: "hello",
  thread_id: id,
  timezone: "Europe/London",
  images: [{ media_type: "image/png", data: "AA==" }],
};
const approval = { request_id: id, action: "approve", thread_id: id };
const auth = { extension_name: id, token: "auth-secret" };
const install = { name: id, url: "https://tool.test/mcp", kind: "mcp" };
const secrets = { secrets: { API_KEY: "test-secret" } };
const prompt = { content: "continue", done: true };
const search = { query: "query", limit: 3 };
const write = { path: id, content: "# Notes" };
const skill = {
  name: id,
  slug: "owner/skill",
  url: "https://skill.test/SKILL.md",
  content: "# Skill",
};

export const wireCases: WireCase[] = [
  { name: "fetchThreads", call: chat.fetchThreads, path: "/api/chat/threads" },
  {
    name: "createThread",
    call: chat.createThread,
    path: "/api/chat/thread/new",
    method: "POST",
  },
  {
    name: "fetchHistory",
    call: () => chat.fetchHistory(id),
    path: `/api/chat/history?thread_id=${query}`,
  },
  {
    name: "fetchHistory omitted",
    call: () => chat.fetchHistory(),
    path: "/api/chat/history",
  },
  {
    name: "fetchHistory null",
    call: () => chat.fetchHistory(null),
    path: "/api/chat/history",
  },
  {
    name: "sendMessage",
    call: () => chat.sendMessage(send),
    path: "/api/chat/send",
    method: "POST",
    body: send,
  },
  {
    name: "submitApproval",
    call: () => chat.submitApproval(approval),
    path: "/api/chat/approval",
    method: "POST",
    body: approval,
  },
  {
    name: "submitAuthToken",
    call: () => chat.submitAuthToken(auth),
    path: "/api/chat/auth-token",
    method: "POST",
    body: auth,
  },
  {
    name: "cancelAuth",
    call: () => chat.cancelAuth({ extension_name: id }),
    path: "/api/chat/auth-cancel",
    method: "POST",
    body: { extension_name: id },
  },
  {
    name: "fetchExtensions",
    call: extensions.fetchExtensions,
    path: "/api/extensions",
  },
  {
    name: "fetchExtensionTools",
    call: extensions.fetchExtensionTools,
    path: "/api/extensions/tools",
  },
  {
    name: "fetchExtensionRegistry",
    call: () => extensions.fetchExtensionRegistry(` ${id} `),
    path: `/api/extensions/registry?query=${query}`,
  },
  {
    name: "fetchExtensionRegistry omitted",
    call: () => extensions.fetchExtensionRegistry(),
    path: "/api/extensions/registry",
  },
  {
    name: "fetchExtensionRegistry blank",
    call: () => extensions.fetchExtensionRegistry("  "),
    path: "/api/extensions/registry",
  },
  {
    name: "installExtension",
    call: () => extensions.installExtension(install),
    path: "/api/extensions/install",
    method: "POST",
    body: install,
  },
  {
    name: "activateExtension",
    call: () => extensions.activateExtension(id),
    path: `/api/extensions/${segment}/activate`,
    method: "POST",
  },
  {
    name: "removeExtension",
    call: () => extensions.removeExtension(id),
    path: `/api/extensions/${segment}/remove`,
    method: "POST",
  },
  {
    name: "fetchExtensionSetup",
    call: () => extensions.fetchExtensionSetup(id),
    path: `/api/extensions/${segment}/setup`,
  },
  {
    name: "submitExtensionSetup",
    call: () => extensions.submitExtensionSetup(id, secrets),
    path: `/api/extensions/${segment}/setup`,
    method: "POST",
    body: secrets,
  },
  {
    name: "fetchGatewayStatusRaw",
    call: gateway.fetchGatewayStatusRaw,
    path: "/api/gateway/status",
    fallback: null,
  },
  {
    name: "fetchGatewayStatus",
    call: gateway.fetchGatewayStatus,
    path: "/api/gateway/status",
    response: { version: "test", total_connections: 2 },
    result: { label: "Connected", detail: "vtest · 2 live browser streams" },
    fallback: { label: "Preview", detail: "Mock gateway unavailable" },
  },
  {
    name: "fetchRuntimeFeatureFlags",
    call: gateway.fetchRuntimeFeatureFlags,
    path: "/api/features",
    response: { route_jobs: true },
    result: { route_jobs: true },
  },
  { name: "fetchJobs", call: jobs.fetchJobs, path: "/api/jobs" },
  {
    name: "fetchJobSummary",
    call: jobs.fetchJobSummary,
    path: "/api/jobs/summary",
  },
  {
    name: "fetchJobDetail",
    call: () => jobs.fetchJobDetail(id),
    path: `/api/jobs/${segment}`,
  },
  {
    name: "fetchJobEvents",
    call: () => jobs.fetchJobEvents(id),
    path: `/api/jobs/${segment}/events`,
  },
  {
    name: "fetchJobFiles",
    call: () => jobs.fetchJobFiles(id),
    path: `/api/jobs/${segment}/files/list`,
  },
  {
    name: "readJobFile",
    call: () => jobs.readJobFile(id, id),
    path: `/api/jobs/${segment}/files/read?path=${query}`,
  },
  {
    name: "restartJob",
    call: () => jobs.restartJob(id),
    path: `/api/jobs/${segment}/restart`,
    method: "POST",
  },
  {
    name: "cancelJob",
    call: () => jobs.cancelJob(id),
    path: `/api/jobs/${segment}/cancel`,
    method: "POST",
  },
  {
    name: "promptJob",
    call: () => jobs.promptJob(id, prompt),
    path: `/api/jobs/${segment}/prompt`,
    method: "POST",
    body: prompt,
  },
  { name: "fetchLogLevel", call: logs.fetchLogLevel, path: "/api/logs/level" },
  {
    name: "setLogLevel",
    call: () => logs.setLogLevel("debug"),
    path: "/api/logs/level",
    method: "PUT",
    body: { level: "debug" },
  },
  {
    name: "fetchMemoryTree",
    call: () => memory.fetchMemoryTree(0),
    path: "/api/memory/tree?depth=0",
  },
  {
    name: "fetchMemoryTree omitted",
    call: () => memory.fetchMemoryTree(),
    path: "/api/memory/tree",
  },
  {
    name: "readMemory",
    call: () => memory.readMemory(id),
    path: `/api/memory/read?path=${query}`,
  },
  {
    name: "searchMemory",
    call: () => memory.searchMemory(search),
    path: "/api/memory/search",
    method: "POST",
    body: search,
  },
  {
    name: "writeMemory",
    call: () => memory.writeMemory(write),
    path: "/api/memory/write",
    method: "POST",
    body: write,
  },
  {
    name: "fetchPairingRequests",
    call: () => pairing.fetchPairingRequests(id),
    path: `/api/pairing/${segment}`,
  },
  {
    name: "approvePairing",
    call: () => pairing.approvePairing(id, "PAIR-1234"),
    path: `/api/pairing/${segment}/approve`,
    method: "POST",
    body: { code: "PAIR-1234" },
  },
  {
    name: "fetchRoutines",
    call: routines.fetchRoutines,
    path: "/api/routines",
  },
  {
    name: "fetchRoutineSummary",
    call: routines.fetchRoutineSummary,
    path: "/api/routines/summary",
  },
  {
    name: "fetchRoutineDetail",
    call: () => routines.fetchRoutineDetail(id),
    path: `/api/routines/${segment}`,
  },
  {
    name: "fetchRoutineRuns",
    call: () => routines.fetchRoutineRuns(id),
    path: `/api/routines/${segment}/runs`,
  },
  {
    name: "triggerRoutine",
    call: () => routines.triggerRoutine(id),
    path: `/api/routines/${segment}/trigger`,
    method: "POST",
  },
  {
    name: "toggleRoutine",
    call: () => routines.toggleRoutine(id, { enabled: false }),
    path: `/api/routines/${segment}/toggle`,
    method: "POST",
    body: { enabled: false },
  },
  {
    name: "toggleRoutine omitted",
    call: () => routines.toggleRoutine(id),
    path: `/api/routines/${segment}/toggle`,
    method: "POST",
    body: {},
  },
  {
    name: "deleteRoutine",
    call: () => routines.deleteRoutine(id),
    path: `/api/routines/${segment}`,
    method: "DELETE",
  },
  { name: "fetchSkills", call: skills.fetchSkills, path: "/api/skills" },
  {
    name: "searchSkills",
    call: () => skills.searchSkills({ query: "query" }),
    path: "/api/skills/search",
    method: "POST",
    body: { query: "query" },
  },
  {
    name: "installSkill",
    call: () => skills.installSkill(skill),
    path: "/api/skills/install",
    method: "POST",
    body: skill,
  },
  {
    name: "removeSkill",
    call: () => skills.removeSkill(id),
    path: `/api/skills/${segment}`,
    method: "DELETE",
  },
];
