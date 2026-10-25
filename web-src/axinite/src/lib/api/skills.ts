/**
 * @file Skills API wrapper for `/api/skills/*`: list, catalogue search, install,
 * and remove. Used by the `/skills` surface.
 */

import { deleteJson, postJson, requestJson } from "@/lib/api/client";
import type {
  ActionResponse,
  SkillInstallRequest,
  SkillListResponse,
  SkillSearchRequest,
  SkillSearchResponse,
} from "@/lib/api/contracts";

/** GET /api/skills: list installed skills. */
export function fetchSkills(): Promise<SkillListResponse> {
  return requestJson<SkillListResponse>("/api/skills");
}

/** POST /api/skills/search: search the skill catalogue. */
export function searchSkills(
  request: SkillSearchRequest
): Promise<SkillSearchResponse> {
  return postJson<SkillSearchResponse, SkillSearchRequest>(
    "/api/skills/search",
    request
  );
}

/** POST /api/skills/install: install a skill. */
export function installSkill(
  request: SkillInstallRequest
): Promise<ActionResponse> {
  return postJson<ActionResponse, SkillInstallRequest>(
    "/api/skills/install",
    request
  );
}

/** DELETE /api/skills/{name}: remove an installed skill. */
export function removeSkill(name: string): Promise<ActionResponse> {
  return deleteJson<ActionResponse>(`/api/skills/${encodeURIComponent(name)}`);
}
