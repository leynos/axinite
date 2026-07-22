import { Popover } from "@kobalte/core/popover";
import { createQuery } from "@tanstack/solid-query";
import type { Accessor, Component } from "solid-js";
import { createSignal, Show } from "solid-js";

import { useFeatureFlags } from "@/lib/feature-flags/runtime";
import { useI18n } from "@/lib/i18n/provider";
import {
  checkTeeStatus,
  fetchTeeReport,
  type TeeReport,
  type TeeStatus,
  teeApiBase,
} from "@/lib/tee";

const REPORT_DATA_LIMIT = 32;

function truncate(value: string): string {
  return value.length > REPORT_DATA_LIMIT
    ? `${value.slice(0, REPORT_DATA_LIMIT)}…`
    : value;
}

type TeeReportLoader = {
  report: Accessor<TeeReport | undefined>;
  reportError: Accessor<boolean>;
  reportLoading: Accessor<boolean>;
  loadReport: () => void;
  copyReport: () => void;
};

// Encapsulate the lazy report fetch and clipboard export. `status` is the live
// query result so its latest data is read when the report is copied.
function createTeeReportLoader(status: {
  data: TeeStatus | null | undefined;
}): TeeReportLoader {
  const [report, setReport] = createSignal<TeeReport>();
  const [reportError, setReportError] = createSignal(false);
  const [reportLoading, setReportLoading] = createSignal(false);

  const loadReport = () => {
    if (report() || reportLoading()) {
      return;
    }
    setReportError(false);
    setReportLoading(true);
    void fetchTeeReport()
      .then((data) => setReport(data))
      .catch(() => setReportError(true))
      .finally(() => setReportLoading(false));
  };

  const copyReport = () => {
    const current = report();
    if (!current || !navigator.clipboard) {
      return;
    }
    const combined = { ...current, ...(status.data ?? {}) };
    void navigator.clipboard.writeText(JSON.stringify(combined, null, 2));
  };

  return { report, reportError, reportLoading, loadReport, copyReport };
}

// The popover body: attestation fields once the report loads, otherwise the
// loading or error placeholder.
const TeeReportPanel: Component<{
  teeStatus: Accessor<TeeStatus>;
  report: Accessor<TeeReport | undefined>;
  reportError: Accessor<boolean>;
  copyReport: () => void;
}> = (props) => {
  const { t } = useI18n();
  const emptyValue = () => t("tee-value-empty");

  return (
    <Show
      fallback={
        <Show
          fallback={<p class="shell-tee__loading">{t("tee-report-loading")}</p>}
          when={props.reportError()}
        >
          <p class="shell-tee__loading">{t("tee-report-error")}</p>
        </Show>
      }
      when={props.report()}
    >
      {(loaded) => (
        <>
          <div class="shell-tee__field">
            <div class="shell-tee__field-label">
              {t("tee-field-image-digest")}
            </div>
            <div class="shell-tee__field-value">
              {props.teeStatus().image_digest || emptyValue()}
            </div>
          </div>
          <div class="shell-tee__field">
            <div class="shell-tee__field-label">
              {t("tee-field-tls-fingerprint")}
            </div>
            <div class="shell-tee__field-value">
              {loaded().tls_certificate_fingerprint || emptyValue()}
            </div>
          </div>
          <div class="shell-tee__field">
            <div class="shell-tee__field-label">
              {t("tee-field-report-data")}
            </div>
            <div class="shell-tee__field-value">
              {loaded().report_data
                ? truncate(loaded().report_data ?? "")
                : emptyValue()}
            </div>
          </div>
          <div class="shell-tee__field">
            <div class="shell-tee__field-label">{t("tee-field-vm-config")}</div>
            <div class="shell-tee__field-value">
              {loaded().vm_config || emptyValue()}
            </div>
          </div>
          <div class="shell-tee__actions">
            <button
              class="shell-tee__copy"
              onClick={props.copyReport}
              type="button"
            >
              {t("tee-copy-report")}
            </button>
          </div>
        </>
      )}
    </Show>
  );
};

export const TeeAttestation = () => {
  const { t } = useI18n();
  const flags = useFeatureFlags();

  const enabled = () =>
    flags.isRouteVisible("surface_tee_attestation") && teeApiBase() !== null;

  const status = createQuery<TeeStatus | null>(() => ({
    queryKey: ["tee", "status"],
    queryFn: checkTeeStatus,
    enabled: enabled(),
    retry: false,
    staleTime: Number.POSITIVE_INFINITY,
  }));

  const { report, reportError, loadReport, copyReport } =
    createTeeReportLoader(status);

  return (
    <Show when={enabled() && status.data}>
      {(teeStatus) => (
        <Popover
          onOpenChange={(open) => {
            if (open) {
              loadReport();
            }
          }}
        >
          <Popover.Trigger
            aria-label={t("tee-shield-label")}
            class="shell-tee__shield"
          >
            <svg
              aria-hidden="true"
              fill="none"
              height="16"
              stroke="currentColor"
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="2"
              viewBox="0 0 24 24"
              width="16"
            >
              <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
            </svg>
          </Popover.Trigger>
          <Popover.Portal>
            <Popover.Content class="shell-tee__popover">
              <Popover.Title class="shell-tee__title">
                {t("tee-popover-title")}
              </Popover.Title>
              <TeeReportPanel
                copyReport={copyReport}
                report={report}
                reportError={reportError}
                teeStatus={teeStatus}
              />
            </Popover.Content>
          </Popover.Portal>
        </Popover>
      )}
    </Show>
  );
};
