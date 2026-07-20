import {
  createMutation,
  createQuery,
  useQueryClient,
} from "@tanstack/solid-query";
import {
  createEffect,
  createMemo,
  createSignal,
  For,
  type Setter,
  Show,
} from "solid-js";
import type { SearchHit } from "@/lib/api/contracts";
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

type MemorySearchProps = {
  t: (key: string) => string;
  query: () => string;
  setQuery: Setter<string>;
  setActivePath: Setter<string | undefined>;
  results: () => SearchHit[];
};

// Search input plus its live result list. Accessors are passed through so the
// input value and results stay reactive without destructuring props.
const MemorySearch = (props: MemorySearchProps) => (
  <>
    <div class="route-sidebar__search">
      <input
        aria-label={props.t("memory-search-label")}
        class="route-sidebar__search-input"
        onInput={(event) => props.setQuery(event.currentTarget.value)}
        placeholder={props.t("memory-search-placeholder")}
        type="text"
        value={props.query()}
      />
    </div>

    <Show when={props.query().trim().length > 0}>
      <div class="catalogue-search__results skills-search__results">
        <For each={props.results()}>
          {(result) => (
            <button
              class="route-sidebar__list-item"
              onClick={() => {
                props.setActivePath(result.path);
                props.setQuery("");
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
  </>
);

type MemoryTreeProps = {
  groups: () => FileGroup[];
  activePath: () => string | undefined;
  setActivePath: Setter<string | undefined>;
};

// Grouped file tree in the sidebar. The active-path accessor drives the
// highlighted-file class, so it is read through props to preserve reactivity.
const MemoryTree = (props: MemoryTreeProps) => (
  <div class="route-tree">
    <For each={props.groups()}>
      {(group) => (
        <section class="route-tree__group">
          <h3 class="route-tree__folder-title">{group.label}</h3>
          <div class="route-tree__group-items">
            <For each={group.files}>
              {(path) => (
                <button
                  class={
                    props.activePath() === path
                      ? "route-tree__file route-tree__file--active"
                      : "route-tree__file"
                  }
                  onClick={() => props.setActivePath(path)}
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
);

type MemoryDocumentProps = {
  t: (key: string) => string;
  breadcrumbs: () => string[];
  editing: () => boolean;
  setEditing: Setter<boolean>;
  draft: () => string;
  setDraft: Setter<string>;
  content: () => string | undefined;
  activePath: () => string | undefined;
  onSave: () => void;
};

// Main document pane: breadcrumb, edit toolbar, and the read/edit view. All
// state is passed as accessors and setters so the pane mirrors the parent
// signals exactly.
const MemoryDocument = (props: MemoryDocumentProps) => (
  <main class="memory-preview__main">
    <div class="memory-preview__toolbar">
      <div class="memory-preview__breadcrumb">
        <For each={props.breadcrumbs()}>
          {(segment, index) => (
            <>
              <Show when={index() > 0}>
                <span class="memory-preview__breadcrumb-sep">/</span>
              </Show>
              <span
                class={
                  index() === props.breadcrumbs().length - 1
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
          when={props.editing()}
          fallback={
            <button
              class="memory-preview__action-button"
              onClick={() => props.setEditing(true)}
              type="button"
            >
              {props.t("memory-edit-button")}
            </button>
          }
        >
          <button
            class="memory-preview__action-button"
            onClick={() => {
              props.setDraft(props.content() ?? "");
              props.setEditing(false);
            }}
            type="button"
          >
            {props.t("memory-cancel-button")}
          </button>
          <button
            class="memory-preview__save-button"
            onClick={() => props.onSave()}
            type="button"
          >
            {props.t("memory-save-button")}
          </button>
        </Show>
      </div>
    </div>

    <Show
      when={!props.editing()}
      fallback={
        <div class="memory-preview__editor-wrap">
          <textarea
            class="memory-preview__editor"
            onInput={(event) => props.setDraft(event.currentTarget.value)}
            value={props.draft()}
          />
        </div>
      }
    >
      <article class="memory-preview__document">
        <h2 class="memory-preview__document-title">
          {props.activePath()?.split("/").at(-1)}
        </h2>
        <For each={(props.content() ?? "").split("\n\n")}>
          {(paragraph) => (
            <Show when={paragraph.trim().length > 0}>
              <p class="memory-preview__document-paragraph">{paragraph}</p>
            </Show>
          )}
        </For>
      </article>
    </Show>
  </main>
);

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
          <MemorySearch
            query={query}
            results={() => searchResults.data?.results ?? []}
            setActivePath={setActivePath}
            setQuery={setQuery}
            t={t}
          />

          <MemoryTree
            activePath={activePath}
            groups={groups}
            setActivePath={setActivePath}
          />
        </aside>

        <MemoryDocument
          activePath={activePath}
          breadcrumbs={breadcrumbs}
          content={() => document.data?.content}
          draft={draft}
          editing={editing}
          onSave={() => saveMutation.mutate()}
          setDraft={setDraft}
          setEditing={setEditing}
          t={t}
        />
      </div>
    </section>
  );
};
