import { For, Show } from "solid-js";

import { ChatPreview } from "@/components/chat-preview";
import { ExtensionsPreview } from "@/components/extensions-preview";
import { JobsPreview } from "@/components/jobs-preview";
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

export const RoutePage = (props: RoutePageProps) => {
  const flags = useFeatureFlags();
  const { t } = useI18n();
  const details = () => ROUTE_DETAILS[props.routeId];
  const renderGenericPreview = () => (
    <>
      <header class="route-page__hero">
        <div class="route-page__eyebrow">{t("route-hero-eyebrow")}</div>
        <div class="route-page__hero-grid">
          <div>
            <h2 class="route-page__title">
              {t(`route-${props.routeId}-label`)}
            </h2>
            <p class="route-page__summary">
              {t(`page-${props.routeId}-summary`)}
            </p>
          </div>
          <div class="route-page__meta">
            <div class="route-page__meta-card">
              <p class="route-page__meta-label">{t("route-meta-mode-label")}</p>
              <p class="route-page__meta-value">
                {t(`page-${props.routeId}-mode`)}
              </p>
            </div>
            <div class="route-page__meta-card">
              <p class="route-page__meta-label">
                {t("route-meta-status-label")}
              </p>
              <p class="route-page__meta-value">
                {t(`page-${props.routeId}-status`)}
              </p>
            </div>
          </div>
        </div>
      </header>

      <div class="route-page__cards">
        <For each={details().cardKeys}>
          {(cardKey) => (
            <article class="route-card">
              <p class="route-card__kicker">{t("route-card-kicker")}</p>
              <h3 class="route-card__title">
                {t(`page-${props.routeId}-${cardKey}-title`)}
              </h3>
              <p class="route-card__body">
                {t(`page-${props.routeId}-${cardKey}-body`)}
              </p>
            </article>
          )}
        </For>
      </div>

      <section class="route-page__agenda">
        <div>
          <h3 class="route-page__section-title">{t("route-agenda-title")}</h3>
          <p class="route-page__section-body">
            {t(`page-${props.routeId}-agenda`)}
          </p>
        </div>
        <div>
          <h3 class="route-page__section-title">
            {t("route-guardrail-title")}
          </h3>
          <p class="route-page__section-body">
            {t(`page-${props.routeId}-guardrail`)}
          </p>
        </div>
      </section>
    </>
  );

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
        {props.routeId === "chat" ? (
          <ChatPreview />
        ) : props.routeId === "memory" ? (
          <MemoryPreview />
        ) : props.routeId === "jobs" ? (
          <JobsPreview />
        ) : props.routeId === "routines" ? (
          <RoutinesPreview />
        ) : props.routeId === "extensions" ? (
          <ExtensionsPreview />
        ) : props.routeId === "skills" ? (
          <SkillsPreview />
        ) : (
          renderGenericPreview()
        )}
      </Show>
    </section>
  );
};
