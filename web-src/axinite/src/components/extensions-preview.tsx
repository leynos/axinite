/**
 * @file The `/extensions` surface: installed extensions, registry search, install,
 * activate, remove, and secret setup, backed by `lib/api/extensions`. WASM
 * channels also get the activation stepper and pairing queue.
 */

import { AlertDialog } from "@kobalte/core/alert-dialog";
import {
  createMutation,
  createQuery,
  keepPreviousData,
  useQueryClient,
} from "@tanstack/solid-query";
import {
  createEffect,
  createMemo,
  createSignal,
  For,
  onCleanup,
  Show,
} from "solid-js";
import { ExtensionPairing } from "@/components/extension-pairing";
import { WasmChannelStepper } from "@/components/wasm-channel-stepper";
import type {
  EndpointFieldInfo,
  ExtensionInfo,
  ExtensionSetupRequest,
  SecretFieldInfo,
} from "@/lib/api/contracts";
import {
  activateExtension,
  fetchExtensionRegistry,
  fetchExtensionSetup,
  fetchExtensions,
  fetchExtensionTools,
  installExtension,
  removeExtension,
  submitExtensionSetup,
} from "@/lib/api/extensions";
import { useI18n } from "@/lib/i18n/provider";
import { capitalize, pascalCase } from "@/lib/string-case";

// Translation accessor mirroring the shape returned by `useI18n`. Subcomponents
// receive it explicitly so their JSX stays reactive without reaching into the
// parent scope.
type TranslateFn = (
  key: string,
  options?: Record<string, string | number>
) => string;

const KIND_CLASS = new Map<string, string>([
  ["grpcm", "pill pill--info"],
  ["mcp", "pill pill--success"],
  ["mcp_server", "pill pill--success"],
  ["wasm", "pill pill--warning"],
  ["wasm_tool", "pill pill--warning"],
  ["wasm_channel", "pill pill--warning"],
]);

const STATUS_CLASS = new Map<string, string>([
  ["active", "catalogue-status-dot catalogue-status-dot--active"],
  ["inactive", "catalogue-status-dot"],
]);

function isMcpExtensionKind(kind: string): boolean {
  return kind === "mcp" || kind === "mcp_server";
}

function isWasmExtensionKind(kind: string): boolean {
  return kind === "wasm" || kind === "wasm_tool" || kind === "wasm_channel";
}

function kindMatchesRegistry(
  extensionKind: string,
  registryKind: string
): boolean {
  if (extensionKind === registryKind) {
    return true;
  }

  return (
    (isMcpExtensionKind(extensionKind) && isMcpExtensionKind(registryKind)) ||
    (isWasmExtensionKind(extensionKind) && isWasmExtensionKind(registryKind))
  );
}

function tagList(extension: ExtensionInfo): string[] {
  const tags = [
    extension.active ? "actions" : "read",
    extension.authenticated ? "read_write" : "triggers",
  ];
  if (extension.has_auth) {
    tags.push("events");
  }
  return tags;
}

function tagLabel(t: TranslateFn, tag: string): string {
  return t(
    `extensions-tag-${pascalCase(tag)
      .replaceAll(/([A-Z])/g, "-$1")
      .toLowerCase()
      .replace(/^-/, "")}`
  );
}

function extensionDisplayName(extension: ExtensionInfo): string {
  return extension.display_name ?? extension.name;
}

function extensionKindLabel(t: TranslateFn, kind: string): string {
  if (isMcpExtensionKind(kind)) {
    return t("extensions-kind-mcp");
  }

  if (isWasmExtensionKind(kind)) {
    return t("extensions-kind-wasm");
  }

  return capitalize(kind).toLowerCase();
}

function appendKeyCell(row: HTMLTableRowElement, text: string) {
  const cell = document.createElement("td");
  cell.className = "catalogue-list__key";
  cell.textContent = text;
  row.append(cell);
}

type TextCellSpec = {
  className: string;
  textClassName: string;
  text: string;
};

function appendTextCell(row: HTMLTableRowElement, spec: TextCellSpec) {
  const cell = document.createElement("td");
  cell.className = spec.className;
  const paragraph = document.createElement("p");
  paragraph.className = spec.textClassName;
  paragraph.textContent = spec.text;
  cell.append(paragraph);
  row.append(cell);
}

function renderRows(
  body: HTMLTableSectionElement | undefined,
  buildRows: () => HTMLTableRowElement[]
) {
  if (!body) {
    return;
  }

  body.replaceChildren(...buildRows());
}

function useTableBodyCleanup(
  getBody: () => HTMLTableSectionElement | undefined
) {
  onCleanup(() => {
    getBody()?.replaceChildren();
  });
}

type InstalledExtensionCardProps = {
  extension: ExtensionInfo;
  t: TranslateFn;
  onConfigure: (name: string) => void;
  onToggleActive: (name: string) => void;
  onRequestRemove: (name: string) => void;
  onPairingApproved: () => void;
};

function InstalledExtensionCard(props: InstalledExtensionCardProps) {
  return (
    <article class="catalogue-card extensions-card">
      <div class="catalogue-card__header">
        <div class="catalogue-card__title-wrap">
          <h4 class="catalogue-card__title">
            {extensionDisplayName(props.extension)}
          </h4>
          <span
            class={KIND_CLASS.get(props.extension.kind) ?? "pill pill--neutral"}
          >
            {extensionKindLabel(props.t, props.extension.kind)}
          </span>
        </div>
        <div class="catalogue-card__meta">
          <span>
            {props.extension.version ?? props.t("extensions-version-preview")}
          </span>
          <span
            class={
              props.extension.active
                ? STATUS_CLASS.get("active")
                : STATUS_CLASS.get("inactive")
            }
          />
        </div>
      </div>

      <p class="catalogue-card__path">
        {props.extension.url ?? props.t("extensions-url-local")}
      </p>
      <p class="catalogue-card__body">{props.extension.description}</p>

      <Show when={props.extension.kind === "wasm_channel"}>
        <WasmChannelStepper
          activationStatus={props.extension.activation_status}
        />
      </Show>

      <div class="catalogue-card__tags">
        <For each={tagList(props.extension)}>
          {(tag) => (
            <span class="pill pill--neutral">{tagLabel(props.t, tag)}</span>
          )}
        </For>
      </div>

      <div class="catalogue-card__actions">
        <button
          class="catalogue-card__action"
          onClick={() => props.onConfigure(props.extension.name)}
          type="button"
        >
          {props.t("extensions-action-configure")}
        </button>
        <button
          class="catalogue-card__action"
          onClick={() => props.onToggleActive(props.extension.name)}
          type="button"
        >
          {props.extension.active
            ? props.t("extensions-action-disable")
            : props.t("extensions-action-activate")}
        </button>
        <button
          aria-label={props.t("extensions-action-remove-label", {
            name: extensionDisplayName(props.extension),
          })}
          class="catalogue-card__action"
          onClick={() => props.onRequestRemove(props.extension.name)}
          type="button"
        >
          {props.t("extensions-action-remove")}
        </button>
      </div>

      <Show when={props.extension.kind === "wasm_channel"}>
        <ExtensionPairing
          channel={props.extension.name}
          onApproved={props.onPairingApproved}
        />
      </Show>
    </article>
  );
}

type ConfigurePanelProps = {
  name: string;
  secrets: SecretFieldInfo[];
  endpoints: EndpointFieldInfo[];
  endpointValues: Record<string, string>;
  endpointClears: Record<string, boolean>;
  resultMessage: string | undefined;
  t: TranslateFn;
  onSecretChange: (fieldName: string, value: string) => void;
  onEndpointChange: (fieldName: string, value: string) => void;
  onEndpointClear: (fieldName: string, checked: boolean) => void;
  onSave: () => void;
  onCancel: () => void;
};

/** Placeholder that tells the user what the stored endpoint state means. */
function endpointPlaceholder(field: EndpointFieldInfo, t: TranslateFn): string {
  if (field.state === "configured") {
    return t("extensions-setup-endpoint-replace-hint");
  }
  if (field.state === "stale" || field.state === "corrupt") {
    return t("extensions-setup-endpoint-attention");
  }
  return field.prompt;
}

/** Endpoint replacements to submit: non-empty and not marked for clearing. */
function endpointSetValues(
  values: Record<string, string>,
  clears: Record<string, boolean>
) {
  const set: Record<string, string> = {};
  for (const [name, value] of Object.entries(values)) {
    const trimmed = value.trim();
    if (trimmed.length > 0 && clears[name] !== true) {
      set[name] = trimmed;
    }
  }
  return set;
}

/** Endpoint names marked for an explicit clear. */
function clearedEndpointNames(clears: Record<string, boolean>): string[] {
  return Object.entries(clears)
    .filter(([, cleared]) => cleared)
    .map(([name]) => name);
}

type SecretFieldProps = {
  field: SecretFieldInfo;
  onSecretChange: (fieldName: string, value: string) => void;
  t: TranslateFn;
};

/** One masked credential input with its stored-value indicator. */
function SecretField(props: SecretFieldProps) {
  return (
    <div class="catalogue-form">
      <label class="catalogue-form__label" for={`setup-${props.field.name}`}>
        {props.field.prompt}
      </label>
      <div class="catalogue-form__row catalogue-form__row--indicator">
        <input
          class="catalogue-form__input"
          id={`setup-${props.field.name}`}
          onInput={(event) =>
            props.onSecretChange(props.field.name, event.currentTarget.value)
          }
          placeholder={
            props.field.provided
              ? props.t("extensions-setup-provided-hint")
              : props.field.prompt
          }
          type="text"
        />
        <Show when={props.field.provided}>
          <span
            aria-label={props.t("extensions-setup-stored")}
            class="catalogue-form__provided-icon"
            role="img"
          >
            ✓
          </span>
        </Show>
      </div>
    </div>
  );
}

type EndpointFieldProps = {
  clears: Record<string, boolean>;
  field: EndpointFieldInfo;
  onEndpointChange: (fieldName: string, value: string) => void;
  onEndpointClear: (fieldName: string, checked: boolean) => void;
  t: TranslateFn;
  values: Record<string, string>;
};

/**
 * One confidential endpoint input with its clear control and status.
 *
 * The stored URL is never rendered: only its state appears, so a reload cannot
 * leak the value back to the browser.
 */
function EndpointField(props: EndpointFieldProps) {
  const cleared = () => props.clears[props.field.name] === true;
  return (
    <div class="catalogue-form">
      <label
        class="catalogue-form__label"
        for={`setup-endpoint-${props.field.name}`}
      >
        {props.field.prompt}
        <Show when={props.field.optional}>
          <span class="catalogue-form__optional">
            {" "}
            {props.t("extensions-setup-optional")}
          </span>
        </Show>
      </label>
      <div class="catalogue-form__row catalogue-form__row--indicator">
        <input
          autocomplete="off"
          class="catalogue-form__input"
          disabled={cleared()}
          id={`setup-endpoint-${props.field.name}`}
          inputmode="url"
          onInput={(event) =>
            props.onEndpointChange(props.field.name, event.currentTarget.value)
          }
          placeholder={endpointPlaceholder(props.field, props.t)}
          spellcheck={false}
          type="url"
          value={props.values[props.field.name] ?? ""}
        />
        <Show when={props.field.state === "configured"}>
          <span
            aria-label={props.t("extensions-setup-stored")}
            class="catalogue-form__provided-icon"
            role="img"
          >
            ✓
          </span>
        </Show>
      </div>
      <Show when={props.field.state !== "missing"}>
        <label class="catalogue-form__check">
          <input
            checked={cleared()}
            onChange={(event) =>
              props.onEndpointClear(
                props.field.name,
                event.currentTarget.checked
              )
            }
            type="checkbox"
          />
          {props.t("extensions-setup-endpoint-clear")}
        </label>
      </Show>
      <Show
        when={props.field.state === "stale" || props.field.state === "corrupt"}
      >
        <p class="catalogue-form__warning" role="status">
          {props.t("extensions-setup-endpoint-attention")}
        </p>
      </Show>
    </div>
  );
}

function ConfigurePanel(props: ConfigurePanelProps) {
  return (
    <aside class="catalogue-detail catalogue-detail--inline extensions-detail">
      <h3 class="catalogue-detail__title">
        {props.t("extensions-configure-title", {
          name: props.name,
        })}
      </h3>

      <For each={props.secrets}>
        {(field) => (
          <SecretField
            field={field}
            onSecretChange={props.onSecretChange}
            t={props.t}
          />
        )}
      </For>

      <For each={props.endpoints}>
        {(field) => (
          <EndpointField
            clears={props.endpointClears}
            field={field}
            onEndpointChange={props.onEndpointChange}
            onEndpointClear={props.onEndpointClear}
            t={props.t}
            values={props.endpointValues}
          />
        )}
      </For>

      <Show when={props.secrets.length === 0 && props.endpoints.length === 0}>
        <p class="catalogue-panel__empty">{props.t("extensions-setup-none")}</p>
      </Show>

      <Show when={props.resultMessage}>
        <p aria-live="polite" class="catalogue-form__warning" role="status">
          {props.resultMessage}
        </p>
      </Show>

      <div class="dashboard-detail__actions">
        <button
          class="dashboard-detail__ghost"
          type="button"
          onClick={() => props.onSave()}
        >
          {props.t("extensions-action-save")}
        </button>
        <button
          class="dashboard-detail__ghost dashboard-detail__ghost--danger"
          type="button"
          onClick={() => props.onCancel()}
        >
          {props.t("extensions-action-cancel")}
        </button>
      </div>
    </aside>
  );
}

type RegistrySearchPanelProps = {
  t: TranslateFn;
  query: string;
  onQueryInput: (value: string) => void;
  bodyRef: (element: HTMLTableSectionElement) => void;
};

function RegistrySearchPanel(props: RegistrySearchPanelProps) {
  return (
    <section class="catalogue-panel">
      <div class="catalogue-panel__mark">{props.t("extensions-wasm-mark")}</div>
      <div class="catalogue-panel__content">
        <h3 class="catalogue-panel__title">
          {props.t("extensions-wasm-title")}
        </h3>
        <div class="catalogue-form">
          <label class="catalogue-form__label" for="extensions-registry-search">
            {props.t("extensions-registry-label")}
          </label>
          <div class="catalogue-form__row">
            <input
              class="catalogue-form__input"
              id="extensions-registry-search"
              onInput={(event) => props.onQueryInput(event.currentTarget.value)}
              placeholder={props.t("extensions-wasm-placeholder")}
              type="text"
              value={props.query}
            />
          </div>
        </div>
        <div class="catalogue-table-wrap">
          <table class="catalogue-list catalogue-list--extensions">
            <caption class="catalogue-table__caption">
              {props.t("extensions-wasm-title")}
            </caption>
            <thead>
              <tr class="catalogue-list__row">
                <th class="catalogue-list__content" scope="col">
                  {props.t("routines-column-name")}
                </th>
                <th class="catalogue-list__content" scope="col">
                  {props.t("extensions-column-description")}
                </th>
                <th class="catalogue-list__action" scope="col">
                  {props.t("routines-column-action")}
                </th>
              </tr>
            </thead>
            <tbody ref={props.bodyRef} />
          </table>
        </div>
      </div>
    </section>
  );
}

type McpServerPanelProps = {
  t: TranslateFn;
  hasExtensions: boolean;
  serverName: string;
  onServerNameInput: (value: string) => void;
  onAddServer: () => void;
  bodyRef: (element: HTMLTableSectionElement) => void;
};

function McpServerPanel(props: McpServerPanelProps) {
  return (
    <section class="catalogue-panel">
      <div class="catalogue-panel__mark">{props.t("extensions-mcp-mark")}</div>
      <div class="catalogue-panel__content">
        <h3 class="catalogue-panel__title">
          {props.t("extensions-mcp-title")}
        </h3>
        <Show
          when={props.hasExtensions}
          fallback={
            <p class="catalogue-panel__empty">
              {props.t("extensions-mcp-empty")}
            </p>
          }
        >
          <div class="catalogue-table-wrap">
            <table class="catalogue-list catalogue-list--extensions">
              <caption class="catalogue-table__caption">
                {props.t("extensions-mcp-title")}
              </caption>
              <thead>
                <tr class="catalogue-list__row">
                  <th class="catalogue-list__key" scope="col">
                    {props.t("routines-column-name")}
                  </th>
                  <th class="catalogue-list__content" scope="col">
                    {props.t("extensions-column-endpoint")}
                  </th>
                </tr>
              </thead>
              <tbody ref={props.bodyRef} />
            </table>
          </div>
        </Show>
        <h4 class="catalogue-panel__subtitle">
          {props.t("extensions-mcp-add-title")}
        </h4>
        <div class="catalogue-form">
          <label class="catalogue-form__label" for="mcp-server-name">
            {props.t("extensions-mcp-field-label")}
          </label>
          <div class="catalogue-form__row">
            <input
              class="catalogue-form__input"
              id="mcp-server-name"
              onInput={(event) =>
                props.onServerNameInput(event.currentTarget.value)
              }
              placeholder={props.t("extensions-mcp-placeholder")}
              type="text"
              value={props.serverName}
            />
          </div>
        </div>
        <button
          class="catalogue-form__button"
          type="button"
          onClick={() => props.onAddServer()}
        >
          {props.t("extensions-mcp-action")}
        </button>
      </div>
    </section>
  );
}

type ExtensionToolsSectionProps = {
  t: TranslateFn;
  bodyRef: (element: HTMLTableSectionElement) => void;
};

function ExtensionToolsSection(props: ExtensionToolsSectionProps) {
  return (
    <section class="catalogue-section catalogue-section--bare">
      <div class="catalogue-section__header extensions-preview__section-header">
        <div>
          <h3 class="catalogue-section__title">
            {props.t("extensions-tools-title")}
          </h3>
          <p class="catalogue-section__body">
            {props.t("page-extensions-guardrail")}
          </p>
        </div>
      </div>

      <div class="catalogue-table-wrap">
        <table class="catalogue-list catalogue-list--extensions">
          <caption class="catalogue-table__caption">
            {props.t("extensions-tools-title")}
          </caption>
          <thead>
            <tr class="catalogue-list__row">
              <th class="catalogue-list__key" scope="col">
                {props.t("extensions-column-tool")}
              </th>
              <th class="catalogue-list__content" scope="col">
                {props.t("extensions-tools-source-label")}
              </th>
              <th class="catalogue-list__content" scope="col">
                {props.t("extensions-column-description")}
              </th>
            </tr>
          </thead>
          <tbody ref={props.bodyRef} />
        </table>
      </div>
    </section>
  );
}

type RemoveExtensionDialogProps = {
  t: TranslateFn;
  extension: ExtensionInfo | null;
  showReinstallHint: boolean;
  isRemoving: boolean;
  onOpenChange: (open: boolean) => void;
  onConfirm: (name: string) => void;
};

function RemoveExtensionDialog(props: RemoveExtensionDialogProps) {
  return (
    <AlertDialog
      onOpenChange={(open) => props.onOpenChange(open)}
      open={props.extension !== null}
    >
      <AlertDialog.Portal>
        <AlertDialog.Overlay class="dialog-overlay" />
        <Show when={props.extension}>
          {(extension) => (
            <AlertDialog.Content class="dialog-surface extensions-remove-dialog">
              <AlertDialog.Title class="dialog-title">
                {props.t("extensions-remove-title", {
                  name: extensionDisplayName(extension()),
                })}
              </AlertDialog.Title>
              <AlertDialog.Description class="dialog-description">
                {props.t("extensions-remove-description", {
                  name: extensionDisplayName(extension()),
                })}
              </AlertDialog.Description>
              <Show when={props.showReinstallHint}>
                <p class="dialog-description">
                  {props.t("extensions-remove-reinstall-hint")}
                </p>
              </Show>
              <div class="dashboard-detail__actions extensions-remove-dialog__actions">
                <button
                  class="dashboard-detail__ghost"
                  disabled={props.isRemoving}
                  onClick={() => props.onConfirm(extension().name)}
                  type="button"
                >
                  {props.t("extensions-remove-confirm")}
                </button>
                <AlertDialog.CloseButton
                  class="dashboard-detail__ghost dashboard-detail__ghost--danger"
                  disabled={props.isRemoving}
                >
                  {props.t("extensions-action-cancel")}
                </AlertDialog.CloseButton>
              </div>
            </AlertDialog.Content>
          )}
        </Show>
      </AlertDialog.Portal>
    </AlertDialog>
  );
}

/**
 * Manages extension and MCP discovery, installation, configuration, pairing
 * and removal.
 */
export const ExtensionsPreview = () => {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const [configuringName, setConfiguringName] = createSignal<string>();
  const [pendingRemovalName, setPendingRemovalName] = createSignal<string>();
  const [registryQuery, setRegistryQuery] = createSignal("");
  const [mcpServerName, setMcpServerName] = createSignal("");
  const [setupValues, setSetupValues] = createSignal<Record<string, string>>(
    {}
  );
  const [endpointValues, setEndpointValues] = createSignal<
    Record<string, string>
  >({});
  const [endpointClears, setEndpointClears] = createSignal<
    Record<string, boolean>
  >({});
  const [setupMessage, setSetupMessage] = createSignal<string>();
  let registryBodyRef: HTMLTableSectionElement | undefined;
  let mcpBodyRef: HTMLTableSectionElement | undefined;
  let toolsBodyRef: HTMLTableSectionElement | undefined;

  const extensions = createQuery(() => ({
    queryKey: ["extensions", "list"],
    queryFn: fetchExtensions,
  }));

  const tools = createQuery(() => ({
    queryKey: ["extensions", "tools"],
    queryFn: fetchExtensionTools,
  }));

  const registry = createQuery(() => ({
    queryKey: ["extensions", "registry", registryQuery().trim()],
    queryFn: () => fetchExtensionRegistry(registryQuery().trim()),
  }));

  const mcpExtensions = createMemo(
    () =>
      extensions.data?.extensions.filter((extension) =>
        isMcpExtensionKind(extension.kind)
      ) ?? []
  );

  const activeExtension = createMemo<ExtensionInfo | null>(
    () =>
      extensions.data?.extensions.find(
        (extension) => extension.name === configuringName()
      ) ?? null
  );

  const pendingRemovalExtension = createMemo<ExtensionInfo | null>(
    () =>
      extensions.data?.extensions.find(
        (extension) => extension.name === pendingRemovalName()
      ) ?? null
  );

  const pendingRemovalWasmRegistryEntry = createMemo(() => {
    const extension = pendingRemovalExtension();
    if (!extension) {
      return null;
    }

    return (
      registry.data?.entries.find(
        (entry) =>
          entry.name === extension.name &&
          kindMatchesRegistry(extension.kind, entry.kind) &&
          isWasmExtensionKind(entry.kind)
      ) ?? null
    );
  });

  const setup = createQuery(() => ({
    queryKey: ["extensions", "setup", configuringName()],
    queryFn: () => fetchExtensionSetup(configuringName() ?? ""),
    enabled: configuringName() !== undefined,
    placeholderData: keepPreviousData,
  }));

  createEffect(() => {
    const nextValues = Object.fromEntries(
      (setup.data?.secrets ?? []).map((field) => [field.name, ""])
    );
    setSetupValues(nextValues);
    const nextEndpoints = Object.fromEntries(
      (setup.data?.endpoints ?? []).map((field) => [field.name, ""])
    );
    setEndpointValues(nextEndpoints);
    setEndpointClears({});
    setSetupMessage(undefined);
  });

  const refresh = () => {
    void queryClient.invalidateQueries({ queryKey: ["extensions"] });
  };

  const installMutation = createMutation(() => ({
    mutationFn: (name: string) => installExtension({ name }),
    onSuccess: refresh,
  }));

  const activateMutation = createMutation(() => ({
    mutationFn: (name: string) => activateExtension(name),
    onSuccess: refresh,
  }));

  const removeMutation = createMutation(() => ({
    mutationFn: (name: string) => removeExtension(name),
    onSuccess: (_, name) => {
      if (configuringName() === name) {
        setConfiguringName(undefined);
      }
      setPendingRemovalName(undefined);
      refresh();
    },
  }));

  const setupMutation = createMutation(() => ({
    mutationFn: () => {
      const request: ExtensionSetupRequest = {
        secrets: setupValues(),
        endpoints: {
          set: endpointSetValues(endpointValues(), endpointClears()),
          clear: clearedEndpointNames(endpointClears()),
        },
      };
      return submitExtensionSetup(configuringName() ?? "", request);
    },
    onSuccess: (result) => {
      setSetupMessage(result.message);
      refresh();
    },
    onError: (error) => {
      setSetupMessage(
        error instanceof Error ? error.message : "Failed to save configuration"
      );
    },
  }));

  const addMcpServer = () => {
    const name = mcpServerName().trim();
    if (name) {
      installMutation.mutate(name);
      setMcpServerName("");
    }
  };

  const handleRemoveDialogOpenChange = (open: boolean) => {
    if (!open && !removeMutation.isPending) {
      setPendingRemovalName(undefined);
    }
  };

  createEffect(() => {
    renderRows(registryBodyRef, () =>
      (registry.data?.entries ?? []).map((entry) => {
        const row = document.createElement("tr");
        row.className = "catalogue-list__row";

        appendTextCell(row, {
          className: "catalogue-list__content",
          textClassName: "catalogue-list__source",
          text: entry.display_name,
        });
        appendTextCell(row, {
          className: "catalogue-list__content",
          textClassName: "catalogue-list__body",
          text: entry.description,
        });

        const actionCell = document.createElement("td");
        actionCell.className = "catalogue-list__action";
        const actionButton = document.createElement("button");
        actionButton.className = "catalogue-card__action";
        actionButton.type = "button";
        actionButton.disabled = entry.installed;
        actionButton.textContent = entry.installed
          ? t("extensions-action-installed")
          : t("extensions-action-install");
        actionButton.addEventListener("click", () =>
          installMutation.mutate(entry.name)
        );
        actionCell.append(actionButton);
        row.append(actionCell);

        return row;
      })
    );
  });

  createEffect(() => {
    renderRows(mcpBodyRef, () =>
      mcpExtensions().map((extension) => {
        const row = document.createElement("tr");
        row.className = "catalogue-list__row";
        appendKeyCell(row, extension.display_name ?? extension.name);
        appendTextCell(row, {
          className: "catalogue-list__content",
          textClassName: "catalogue-list__source",
          text: extension.url ?? t("extensions-url-local"),
        });
        return row;
      })
    );
  });

  createEffect(() => {
    renderRows(toolsBodyRef, () =>
      (tools.data?.tools ?? []).map((tool) => {
        const row = document.createElement("tr");
        row.className = "catalogue-list__row";
        appendKeyCell(row, tool.name);
        appendTextCell(row, {
          className: "catalogue-list__content",
          textClassName: "catalogue-list__source",
          text: tool.name.includes("_")
            ? t("extensions-tool-source-mock")
            : t("extensions-tool-source-core"),
        });
        appendTextCell(row, {
          className: "catalogue-list__content",
          textClassName: "catalogue-list__body",
          text: tool.description,
        });
        return row;
      })
    );
  });

  useTableBodyCleanup(() => registryBodyRef);
  useTableBodyCleanup(() => mcpBodyRef);
  useTableBodyCleanup(() => toolsBodyRef);

  return (
    <section class="route-preview route-preview--catalogue route-preview--extensions">
      <div aria-hidden="true" class="route-preview__watermark">
        {t("extensions-watermark")}
      </div>

      <div class="catalogue-preview catalogue-preview--extensions">
        <header class="route-preview__intro catalogue-preview__intro extensions-preview__intro">
          <h2 class="route-preview__title">{t("route-extensions-label")}</h2>
          <p class="route-preview__summary">{t("page-extensions-summary")}</p>
        </header>

        <section class="catalogue-section catalogue-section--bare">
          <div class="catalogue-section__header extensions-preview__section-header">
            <div>
              <h3 class="catalogue-section__title">
                {t("extensions-installed-title")}
              </h3>
              <p class="catalogue-section__body">
                {t("page-extensions-agenda")}
              </p>
            </div>
          </div>

          <div class="catalogue-grid catalogue-grid--extensions">
            <For each={extensions.data?.extensions ?? []}>
              {(extension) => (
                <InstalledExtensionCard
                  extension={extension}
                  onConfigure={(name) => setConfiguringName(name)}
                  onPairingApproved={refresh}
                  onRequestRemove={(name) => setPendingRemovalName(name)}
                  onToggleActive={(name) => activateMutation.mutate(name)}
                  t={t}
                />
              )}
            </For>
          </div>

          <Show when={activeExtension()}>
            {(extension) => (
              <ConfigurePanel
                endpointClears={endpointClears()}
                endpoints={setup.data?.endpoints ?? []}
                endpointValues={endpointValues()}
                name={extension().name}
                onCancel={() => setConfiguringName(undefined)}
                onEndpointChange={(fieldName, value) =>
                  setEndpointValues((current) => ({
                    ...current,
                    [fieldName]: value,
                  }))
                }
                onEndpointClear={(fieldName, checked) =>
                  setEndpointClears((current) => ({
                    ...current,
                    [fieldName]: checked,
                  }))
                }
                onSave={() => setupMutation.mutate()}
                onSecretChange={(fieldName, value) =>
                  setSetupValues((current) => ({
                    ...current,
                    [fieldName]: value,
                  }))
                }
                resultMessage={setupMessage()}
                secrets={setup.data?.secrets ?? []}
                t={t}
              />
            )}
          </Show>
        </section>

        <div class="catalogue-panel-grid catalogue-panel-grid--extensions">
          <RegistrySearchPanel
            bodyRef={(element) => {
              registryBodyRef = element;
            }}
            onQueryInput={(value) => setRegistryQuery(value)}
            query={registryQuery()}
            t={t}
          />

          <McpServerPanel
            bodyRef={(element) => {
              mcpBodyRef = element;
            }}
            hasExtensions={mcpExtensions().length > 0}
            onAddServer={addMcpServer}
            onServerNameInput={(value) => setMcpServerName(value)}
            serverName={mcpServerName()}
            t={t}
          />
        </div>

        <ExtensionToolsSection
          bodyRef={(element) => {
            toolsBodyRef = element;
          }}
          t={t}
        />

        <RemoveExtensionDialog
          extension={pendingRemovalExtension()}
          isRemoving={removeMutation.isPending}
          onConfirm={(name) => removeMutation.mutate(name)}
          onOpenChange={handleRemoveDialogOpenChange}
          showReinstallHint={pendingRemovalWasmRegistryEntry() !== null}
          t={t}
        />
      </div>
    </section>
  );
};
