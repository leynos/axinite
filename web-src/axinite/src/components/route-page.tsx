/**
 * @file Per-route page component used by the router. It shows an unavailable
 * notice when the route's feature flag is off, otherwise the surface for the
 * `RouteId` (chat, memory, jobs, and so on), with a generic preview hero as
 * the fallback.
 */

import type { Component } from "solid-js";
import { Show } from "solid-js";

import { ChatPreview } from "@/components/chat-preview";
import { ExtensionsPreview } from "@/components/extensions-preview";
import { JobsPreview } from "@/components/jobs-preview";
import { LogsPreview } from "@/components/logs-preview";
import { MemoryPreview } from "@/components/memory-preview";
import { RoutinesPreview } from "@/components/routines-preview";
import { SkillsPreview } from "@/components/skills-preview";
import { useFeatureFlags } from "@/lib/feature-flags/runtime";
import { useI18n } from "@/lib/i18n/provider";
import type { RouteId } from "@/lib/route-config";
import { ROUTE_DETAILS } from "@/lib/route-config";

type RoutePageProps = {
  routeId: RouteId;
};

const ROUTE_CONTENT = {
  chat: ChatPreview,
  memory: MemoryPreview,
  jobs: JobsPreview,
  routines: RoutinesPreview,
  extensions: ExtensionsPreview,
  skills: SkillsPreview,
  logs: LogsPreview,
} satisfies Record<RouteId, Component>;

/**
 * Renders the requested route surface or an unavailable notice when its flag
 * is disabled.
 */
export const RoutePage = (props: RoutePageProps) => {
  const flags = useFeatureFlags();
  const { t } = useI18n();
  const details = () => ROUTE_DETAILS[props.routeId];
  const RouteContent = ROUTE_CONTENT[props.routeId];

  return (
    <section class="route-page">
      <Show
        when={flags.isRouteVisible(details().flagName)}
        fallback={
          <div class="route-page__notice">
            <h3>{t("route-unavailable-title")}</h3>
            <p>{t("route-unavailable-body")}</p>
          </div>
        }
      >
        <RouteContent />
      </Show>
    </section>
  );
};
