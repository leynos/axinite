import {
  createMutation,
  createQuery,
  useQueryClient,
} from "@tanstack/solid-query";
import { createEffect, createMemo, createSignal, For, Show } from "solid-js";

import {
  fetchMemoryTree,
  readMemory,
  searchMemory,
  writeMemory,
} from "@/lib/api/memory";
import { useI18n } from "@/lib/i18n/provider";

type FileGroup = {
  label: string;
  files: string[];
};

export const MemoryPreview = () => {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const [activePath, setActivePath] = createSignal<string>();
  const [editing, setEditing] = createSignal(false);
  const [draft, setDraft] = createSignal("");
  const [query, setQuery] = createSignal("");

  const tree = createQuery(() => ({
    queryKey: ["memory", "tree"],
    queryFn: () => fetchMemoryTree(),
  }));

  const searchResults = createQuery(() => ({
    queryKey: ["memory", "search", query().trim()],
    queryFn: () => searchMemory({ query: query().trim(), limit: 8 }),
    enabled: query().trim().length > 0,
  }));

  const filePaths = createMemo(
    () =>
      tree.data?.entries
        .filter((entry) => !entry.is_dir)
        .map((entry) => entry.path)
        .sort((left, right) => left.localeCompare(right)) ?? []
  );

  createEffect(() => {
    const firstPath = filePaths()[0];
    if (!activePath() && firstPath) {
      setActivePath(firstPath);
    }
  });

  const document = createQuery(() => ({
    queryKey: ["memory", "read", activePath()],
    queryFn: () => readMemory(activePath() ?? ""),
    enabled: typeof activePath() === "string",
  }));

  createEffect(() => {
    if (!editing() && document.data?.content) {
      setDraft(document.data.content);
    }
  });

  const saveMutation = createMutation(() => ({
    mutationFn: () =>
      writeMemory({
        path: activePath() ?? "",
        content: draft(),
      }),
    onSuccess: () => {
      setEditing(false);
      void queryClient.invalidateQueries({ queryKey: ["memory", "tree"] });
      void queryClient.invalidateQueries({
        queryKey: ["memory", "read", activePath()],
      });
    },
  }));

  const groups = createMemo<FileGroup[]>(() => {
    const byGroup = new Map<string, string[]>();
    for (const path of filePaths()) {
      const parts = path.split("/");
      const group = parts.length > 2 ? parts[parts.length - 2] : "workspace";
      const current = byGroup.get(group) ?? [];
      current.push(path);
      byGroup.set(group, current);
    }
    return [...byGroup.entries()].map(([label, files]) => ({
      label,
      files,
    }));
  });

  const breadcrumbs = createMemo(() =>
    activePath() ? (activePath()?.split("/") ?? []) : []
  );

  return (
    <section class="route-preview route-preview--memory">
      <div aria-hidden="true" class="route-preview__watermark">
        {t("memory-watermark")}
      </div>
      <div class="route-preview__layout route-preview__layout--memory">
        <aside class="route-sidebar route-sidebar--memory">
          <div class="route-sidebar__search">
            <input
              aria-label={t("memory-search-label")}
              class="route-sidebar__search-input"
              onInput={(event) => setQuery(event.currentTarget.value)}
              placeholder={t("memory-search-placeholder")}
              type="text"
              value={query()}
            />
          </div>

          <Show when={query().trim().length > 0}>
            <div class="catalogue-search__results skills-search__results">
              <For each={searchResults.data?.results ?? []}>
                {(result) => (
                  <button
                    class="route-sidebar__list-item"
                    onClick={() => {
                      setActivePath(result.path);
                      setQuery("");
                    }}
                    type="button"
                  >
                    <span class="route-sidebar__list-label">{result.path}</span>
                    <span class="route-sidebar__list-time">
                      {(result.score * 100).toFixed(0)}%
                    </span>
                  </button>
                )}
              </For>
            </div>
          </Show>

          <div class="route-tree">
            <For each={groups()}>
              {(group) => (
                <section class="route-tree__group">
                  <h3 class="route-tree__folder-title">{group.label}</h3>
                  <div class="route-tree__group-items">
                    <For each={group.files}>
                      {(path) => (
                        <button
                          class={
                            activePath() === path
                              ? "route-tree__file route-tree__file--active"
                              : "route-tree__file"
                          }
                          onClick={() => setActivePath(path)}
                          type="button"
                        >
                          {path.split("/").at(-1)}
                        </button>
                      )}
                    </For>
                  </div>
                </section>
              )}
            </For>
          </div>
        </aside>

        <main class="memory-preview__main">
          <div class="memory-preview__toolbar">
            <div class="memory-preview__breadcrumb">
              <For each={breadcrumbs()}>
                {(segment, index) => (
                  <>
                    <Show when={index() > 0}>
                      <span class="memory-preview__breadcrumb-sep">/</span>
                    </Show>
                    <span
                      class={
                        index() === breadcrumbs().length - 1
                          ? "memory-preview__breadcrumb-current"
                          : "memory-preview__breadcrumb-item"
                      }
                    >
                      {segment}
                    </span>
                  </>
                )}
              </For>
            </div>

            <div class="memory-preview__toolbar-actions">
              <Show
                when={editing()}
                fallback={
                  <button
                    class="memory-preview__action-button"
                    onClick={() => setEditing(true)}
                    type="button"
                  >
                    {t("memory-edit-button")}
                  </button>
                }
              >
                <button
                  class="memory-preview__action-button"
                  onClick={() => {
                    setDraft(document.data?.content ?? "");
                    setEditing(false);
                  }}
                  type="button"
                >
                  {t("memory-cancel-button")}
                </button>
                <button
                  class="memory-preview__save-button"
                  onClick={() => saveMutation.mutate()}
                  type="button"
                >
                  {t("memory-save-button")}
                </button>
              </Show>
            </div>
          </div>

          <Show
            when={!editing()}
            fallback={
              <div class="memory-preview__editor-wrap">
                <textarea
                  class="memory-preview__editor"
                  onInput={(event) => setDraft(event.currentTarget.value)}
                  value={draft()}
                />
              </div>
            }
          >
            <article class="memory-preview__document">
              <h2 class="memory-preview__document-title">
                {activePath()?.split("/").at(-1)}
              </h2>
              <For each={(document.data?.content ?? "").split("\n\n")}>
                {(paragraph) => (
                  <Show when={paragraph.trim().length > 0}>
                    <p class="memory-preview__document-paragraph">
                      {paragraph}
                    </p>
                  </Show>
                )}
              </For>
            </article>
          </Show>
        </main>
      </div>
    </section>
  );
};
