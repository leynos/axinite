import type { JobDetailResponse, JobInfo } from "@/lib/api/contracts";
import { pascalCase } from "@/lib/string-case";

export const STATUS_CLASS: Record<string, string> = {
  completed: "pill pill--success",
  failed: "pill pill--danger",
  in_progress: "pill pill--success",
  pending: "pill pill--neutral",
  stuck: "pill pill--warning",
};

export const SOURCE_CLASS: Record<string, string> = {
  agent: "pill pill--neutral",
  direct: "pill pill--neutral",
  sandbox: "pill pill--info",
};

/**
 * The daemon reports `job_kind: "sandbox"` for jobs launched under
 * `SandboxMode::ClaudeCode` (see `handlers/jobs.rs`), and `can_prompt` is only
 * ever true for that branch. The follow-up "done" signal is meaningful for
 * Claude Code prompt queues alone, so the checkbox is gated on this value.
 */
export const CLAUDE_CODE_JOB_KIND = "sandbox";

export function toKebabSegment(value: string): string {
  return pascalCase(value)
    .replace(/([A-Z])/g, "-$1")
    .toLowerCase()
    .replace(/^-/, "");
}

export function formatTimestamp(
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

export function sourceName(
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

/** Collapses whitespace and truncates a payload preview for compact display. */
export function truncatePreview(value: string, limit = 140): string {
  const collapsed = value.replace(/\s+/g, " ").trim();
  if (collapsed.length <= limit) {
    return collapsed;
  }
  return `${collapsed.slice(0, limit)}…`;
}
