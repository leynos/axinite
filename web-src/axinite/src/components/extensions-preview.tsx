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
import type { ExtensionInfo } from "@/lib/api/contracts";
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
import { capitalise, pascalCase } from "@/lib/string-case";

const KIND_CLASS: Record<string, string> = {
  grpcm: "pill pill--info",
  mcp: "pill pill--success",
  mcp_server: "pill pill--success",
  wasm: "pill pill--warning",
  wasm_tool: "pill pill--warning",
  wasm_channel: "pill pill--warning",
};

const STATUS_CLASS: Record<string, string> = {
  active: "catalogue-status-dot catalogue-status-dot--active",
  inactive: "catalogue-status-dot",
};

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

function appendKeyCell(row: HTMLTableRowElement, text: string) {
  const cell = document.createElement("td");
  cell.className = "catalogue-list__key";
  cell.textContent = text;
  row.append(cell);
}

function appendTextCell(
  row: HTMLTableRowElement,
  className: string,
  textClassName: string,
  text: string
) {
  const cell = document.createElement("td");
  cell.className = className;
  const paragraph = document.createElement("p");
  paragraph.className = textClassName;
  paragraph.textContent = text;
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
    enabled: typeof configuringName() === "string",
    placeholderData: keepPreviousData,
  }));

  createEffect(() => {
    const nextValues = Object.fromEntries(
      (setup.data?.secrets ?? []).map((field) => [field.name, ""])
    );
    setSetupValues(nextValues);
  });

  const refresh = () => {
    void queryClient.invalidateQueries({ queryKey: ["extensions"] });
  };

  const installMutation = createMutation(() => ({
    mutationFn: (name: string) => installExtension(name),
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
    mutationFn: () =>
      submitExtensionSetup(configuringName() ?? "", {
        secrets: setupValues(),
      }),
    onSuccess: refresh,
  }));

  const tagList = (extension: ExtensionInfo) => {
    const tags = [
      extension.active ? "actions" : "read",
      extension.authenticated ? "read_write" : "triggers",
    ];
    if (extension.has_auth) {
      tags.push("events");
    }
    return tags;
  };

  const tagLabel = (tag: string) =>
    t(
      `extensions-tag-${pascalCase(tag)
        .replace(/([A-Z])/g, "-$1")
        .toLowerCase()
        .replace(/^-/, "")}`
    );

  const extensionDisplayName = (extension: ExtensionInfo) =>
    extension.display_name ?? extension.name;

  const extensionKindLabel = (kind: string) => {
    if (isMcpExtensionKind(kind)) {
      return t("extensions-kind-mcp");
    }

    if (isWasmExtensionKind(kind)) {
      return t("extensions-kind-wasm");
    }

    return capitalise(kind).toLowerCase();
  };

  createEffect(() => {
    renderRows(registryBodyRef, () =>
      (registry.data?.entries ?? []).map((entry) => {
        const row = document.createElement("tr");
        row.className = "catalogue-list__row";

        appendTextCell(
          row,
          "catalogue-list__content",
          "catalogue-list__source",
          entry.display_name
        );
        appendTextCell(
          row,
          "catalogue-list__content",
          "catalogue-list__body",
          entry.description
        );

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
        appendTextCell(
          row,
          "catalogue-list__content",
          "catalogue-list__source",
          extension.url ?? t("extensions-url-local")
        );
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
        appendTextCell(
          row,
          "catalogue-list__content",
          "catalogue-list__source",
          tool.name.includes("_")
            ? t("extensions-tool-source-mock")
            : t("extensions-tool-source-core")
        );
        appendTextCell(
          row,
          "catalogue-list__content",
          "catalogue-list__body",
          tool.description
        );
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
                <article class="catalogue-card extensions-card">
                  <div class="catalogue-card__header">
                    <div class="catalogue-card__title-wrap">
                      <h4 class="catalogue-card__title">
                        {extensionDisplayName(extension)}
                      </h4>
                      <span
                        class={
                          KIND_CLASS[extension.kind] ?? "pill pill--neutral"
                        }
                      >
                        {extensionKindLabel(extension.kind)}
                      </span>
                    </div>
                    <div class="catalogue-card__meta">
                      <span>
                        {extension.version ?? t("extensions-version-preview")}
                      </span>
                      <span
                        class={
                          extension.active
                            ? STATUS_CLASS.active
                            : STATUS_CLASS.inactive
                        }
                      />
                    </div>
                  </div>

                  <p class="catalogue-card__path">
                    {extension.url ?? t("extensions-url-local")}
                  </p>
                  <p class="catalogue-card__body">{extension.description}</p>

                  <div class="catalogue-card__tags">
                    <For each={tagList(extension)}>
                      {(tag) => (
                        <span class="pill pill--neutral">{tagLabel(tag)}</span>
                      )}
                    </For>
                  </div>

                  <div class="catalogue-card__actions">
                    <button
                      class="catalogue-card__action"
                      onClick={() => setConfiguringName(extension.name)}
                      type="button"
                    >
                      {t("extensions-action-configure")}
                    </button>
                    <button
                      class="catalogue-card__action"
                      onClick={() => activateMutation.mutate(extension.name)}
                      type="button"
                    >
                      {extension.active
                        ? t("extensions-action-disable")
                        : t("extensions-action-activate")}
                    </button>
                    <button
                      aria-label={t("extensions-action-remove-label", {
                        name: extensionDisplayName(extension),
                      })}
                      class="catalogue-card__action"
                      onClick={() => setPendingRemovalName(extension.name)}
                      type="button"
                    >
                      {t("extensions-action-remove")}
                    </button>
                  </div>
                </article>
              )}
            </For>
          </div>

          <Show when={activeExtension()}>
            {(extension) => (
              <aside class="catalogue-detail catalogue-detail--inline extensions-detail">
                <h3 class="catalogue-detail__title">
                  {t("extensions-configure-title", {
                    name: extension().name,
                  })}
                </h3>

                <For each={setup.data?.secrets ?? []}>
                  {(field) => (
                    <div class="catalogue-form">
                      <label
                        class="catalogue-form__label"
                        for={`setup-${field.name}`}
                      >
                        {field.prompt}
                      </label>
                      <div class="catalogue-form__row catalogue-form__row--indicator">
                        <input
                          class="catalogue-form__input"
                          id={`setup-${field.name}`}
                          onInput={(event) =>
                            setSetupValues((current) => ({
                              ...current,
                              [field.name]: event.currentTarget.value,
                            }))
                          }
                          placeholder={
                            field.provided
                              ? t("extensions-setup-provided-hint")
                              : field.prompt
                          }
                          type="text"
                        />
                        <Show when={field.provided}>
                          <span
                            class="catalogue-form__provided-icon"
                            role="img"
                            aria-label={t("extensions-setup-stored")}
                          >
                            ✓
                          </span>
                        </Show>
                      </div>
                    </div>
                  )}
                </For>

                <Show when={(setup.data?.secrets ?? []).length === 0}>
                  <p class="catalogue-panel__empty">
                    {t("extensions-setup-none")}
                  </p>
                </Show>

                <div class="dashboard-detail__actions">
                  <button
                    class="dashboard-detail__ghost"
                    type="button"
                    onClick={() => setupMutation.mutate()}
                  >
                    {t("extensions-action-save")}
                  </button>
                  <button
                    class="dashboard-detail__ghost dashboard-detail__ghost--danger"
                    type="button"
                    onClick={() => setConfiguringName(undefined)}
                  >
                    {t("extensions-action-cancel")}
                  </button>
                </div>
              </aside>
            )}
          </Show>
        </section>

        <div class="catalogue-panel-grid catalogue-panel-grid--extensions">
          <section class="catalogue-panel">
            <div class="catalogue-panel__mark">{t("extensions-wasm-mark")}</div>
            <div class="catalogue-panel__content">
              <h3 class="catalogue-panel__title">
                {t("extensions-wasm-title")}
              </h3>
              <div class="catalogue-form">
                <label
                  class="catalogue-form__label"
                  for="extensions-registry-search"
                >
                  {t("extensions-registry-label")}
                </label>
                <div class="catalogue-form__row">
                  <input
                    class="catalogue-form__input"
                    id="extensions-registry-search"
                    onInput={(event) =>
                      setRegistryQuery(event.currentTarget.value)
                    }
                    placeholder={t("extensions-wasm-placeholder")}
                    type="text"
                    value={registryQuery()}
                  />
                </div>
              </div>
              <div class="catalogue-table-wrap">
                <table class="catalogue-list catalogue-list--extensions">
                  <caption class="catalogue-table__caption">
                    {t("extensions-wasm-title")}
                  </caption>
                  <thead>
                    <tr class="catalogue-list__row">
                      <th class="catalogue-list__content" scope="col">
                        {t("routines-column-name")}
                      </th>
                      <th class="catalogue-list__content" scope="col">
                        {t("extensions-column-description")}
                      </th>
                      <th class="catalogue-list__action" scope="col">
                        {t("routines-column-action")}
                      </th>
                    </tr>
                  </thead>
                  <tbody ref={registryBodyRef} />
                </table>
              </div>
            </div>
          </section>

          <section class="catalogue-panel">
            <div class="catalogue-panel__mark">{t("extensions-mcp-mark")}</div>
            <div class="catalogue-panel__content">
              <h3 class="catalogue-panel__title">
                {t("extensions-mcp-title")}
              </h3>
              <Show
                when={mcpExtensions().length > 0}
                fallback={
                  <p class="catalogue-panel__empty">
                    {t("extensions-mcp-empty")}
                  </p>
                }
              >
                <div class="catalogue-table-wrap">
                  <table class="catalogue-list catalogue-list--extensions">
                    <caption class="catalogue-table__caption">
                      {t("extensions-mcp-title")}
                    </caption>
                    <thead>
                      <tr class="catalogue-list__row">
                        <th class="catalogue-list__key" scope="col">
                          {t("routines-column-name")}
                        </th>
                        <th class="catalogue-list__content" scope="col">
                          {t("extensions-column-endpoint")}
                        </th>
                      </tr>
                    </thead>
                    <tbody ref={mcpBodyRef} />
                  </table>
                </div>
              </Show>
              <h4 class="catalogue-panel__subtitle">
                {t("extensions-mcp-add-title")}
              </h4>
              <div class="catalogue-form">
                <label class="catalogue-form__label" for="mcp-server-name">
                  {t("extensions-mcp-field-label")}
                </label>
                <div class="catalogue-form__row">
                  <input
                    class="catalogue-form__input"
                    id="mcp-server-name"
                    onInput={(event) =>
                      setMcpServerName(event.currentTarget.value)
                    }
                    placeholder={t("extensions-mcp-placeholder")}
                    type="text"
                    value={mcpServerName()}
                  />
                </div>
              </div>
              <button
                class="catalogue-form__button"
                type="button"
                onClick={() => {
                  const name = mcpServerName().trim();
                  if (name) {
                    installMutation.mutate(name);
                    setMcpServerName("");
                  }
                }}
              >
                {t("extensions-mcp-action")}
              </button>
            </div>
          </section>
        </div>

        <section class="catalogue-section catalogue-section--bare">
          <div class="catalogue-section__header extensions-preview__section-header">
            <div>
              <h3 class="catalogue-section__title">
                {t("extensions-tools-title")}
              </h3>
              <p class="catalogue-section__body">
                {t("page-extensions-guardrail")}
              </p>
            </div>
          </div>

          <div class="catalogue-table-wrap">
            <table class="catalogue-list catalogue-list--extensions">
              <caption class="catalogue-table__caption">
                {t("extensions-tools-title")}
              </caption>
              <thead>
                <tr class="catalogue-list__row">
                  <th class="catalogue-list__key" scope="col">
                    {t("extensions-column-tool")}
                  </th>
                  <th class="catalogue-list__content" scope="col">
                    {t("extensions-tools-source-label")}
                  </th>
                  <th class="catalogue-list__content" scope="col">
                    {t("extensions-column-description")}
                  </th>
                </tr>
              </thead>
              <tbody ref={toolsBodyRef} />
            </table>
          </div>
        </section>

        <AlertDialog
          onOpenChange={(open) => {
            if (!open && !removeMutation.isPending) {
              setPendingRemovalName(undefined);
            }
          }}
          open={pendingRemovalExtension() !== null}
        >
          <AlertDialog.Portal>
            <AlertDialog.Overlay class="dialog-overlay" />
            <Show when={pendingRemovalExtension()}>
              {(extension) => (
                <AlertDialog.Content class="dialog-surface extensions-remove-dialog">
                  <AlertDialog.Title class="dialog-title">
                    {t("extensions-remove-title", {
                      name: extensionDisplayName(extension()),
                    })}
                  </AlertDialog.Title>
                  <AlertDialog.Description class="dialog-description">
                    {t("extensions-remove-description", {
                      name: extensionDisplayName(extension()),
                    })}
                  </AlertDialog.Description>
                  <Show when={pendingRemovalWasmRegistryEntry()}>
                    <p class="dialog-description">
                      {t("extensions-remove-reinstall-hint")}
                    </p>
                  </Show>
                  <div class="dashboard-detail__actions extensions-remove-dialog__actions">
                    <button
                      class="dashboard-detail__ghost"
                      disabled={removeMutation.isPending}
                      onClick={() => removeMutation.mutate(extension().name)}
                      type="button"
                    >
                      {t("extensions-remove-confirm")}
                    </button>
                    <AlertDialog.CloseButton
                      class="dashboard-detail__ghost dashboard-detail__ghost--danger"
                      disabled={removeMutation.isPending}
                    >
                      {t("extensions-action-cancel")}
                    </AlertDialog.CloseButton>
                  </div>
                </AlertDialog.Content>
              )}
            </Show>
          </AlertDialog.Portal>
        </AlertDialog>
      </div>
    </section>
  );
};
