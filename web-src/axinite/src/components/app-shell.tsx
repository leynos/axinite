import { createQuery } from "@tanstack/solid-query";
import { Link, useRouterState } from "@tanstack/solid-router";
import type { ParentComponent } from "solid-js";
import { createMemo, For, Show } from "solid-js";

import { DebugFlagPanel } from "@/components/debug-flag-panel";
import { LocalePicker } from "@/components/locale-picker";
import { LogsDialog } from "@/components/logs-dialog";
import { fetchGatewayStatus } from "@/lib/api/gateway";
import { buildAppPath } from "@/lib/base-path";
import { useFeatureFlags } from "@/lib/feature-flags/runtime";
import { useI18n } from "@/lib/i18n/provider";
import { ROUTE_ORDER } from "@/lib/route-config";

type ShellChromeProps = {
  activePath: string;
  usePlainLinks?: boolean;
};

export const ShellChrome: ParentComponent<ShellChromeProps> = (props) => {
  const { t } = useI18n();
  const flags = useFeatureFlags();
  const basePath = import.meta.env.BASE_URL as string | undefined;
  const gatewayStatus = createQuery(() => ({
    queryKey: ["gateway-status"],
    queryFn: fetchGatewayStatus,
    refetchInterval: 30_000,
  }));

  return (
    <div class="shell-frame">
      <div class="shell-backdrop" />
      <header class="shell-topbar">
        <div class="shell-topbar__leading">
          <div class="shell-topbar__brand">
            <div class="shell-topbar__mark" />
            <div>
              <p class="shell-topbar__eyebrow">{t("shell-eyebrow")}</p>
              <h1 class="shell-topbar__title">{t("shell-title")}</h1>
            </div>
          </div>

          <nav aria-label={t("shell-navigation-label")} class="shell-nav">
            <For each={ROUTE_ORDER}>
              {(routeId) =>
                props.usePlainLinks ? (
                  <a
                    class={
                      props.activePath === `/${routeId}`
                        ? "shell-nav__link shell-nav__link--active"
                        : "shell-nav__link"
                    }
                    href={buildAppPath(basePath, routeId)}
                  >
                    {t(`route-${routeId}-label`)}
                  </a>
                ) : (
                  <Link
                    to={`/${routeId}`}
                    activeProps={{
                      class: "shell-nav__link shell-nav__link--active",
                    }}
                    inactiveProps={{ class: "shell-nav__link" }}
                  >
                    {t(`route-${routeId}-label`)}
                  </Link>
                )
              }
            </For>
          </nav>
        </div>
        <div class="shell-controls">
          <LocalePicker />
          <Show when={flags.isRouteVisible("panel_logs")}>
            <LogsDialog />
          </Show>
          <div class="shell-status">
            <span class="shell-status__dot" />
            <div>
              <div class="shell-status__label">
                {gatewayStatus.data?.label ?? t("status-preview-label")}
              </div>
              <div class="shell-status__value">
                {gatewayStatus.data?.detail ?? t("status-preview-detail")}
              </div>
            </div>
          </div>
        </div>
      </header>

      <main class="shell-main" data-route={props.activePath}>
        {props.children}
      </main>

      <Show when={flags.isDebugEnabled()}>
        <DebugFlagPanel />
      </Show>
    </div>
  );
};

export const AppShell: ParentComponent = (props) => {
  const routerState = useRouterState();
  const activePath = createMemo(() => routerState().location.pathname);

  return <ShellChrome activePath={activePath()}>{props.children}</ShellChrome>;
};
