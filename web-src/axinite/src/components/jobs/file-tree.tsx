import { createSignal, For, Show } from "solid-js";

import type { ProjectFileEntry } from "@/lib/api/contracts";

export type FileTreeNode = {
  name: string;
  path: string;
  isDir: boolean;
  children: FileTreeNode[];
};

/**
 * Builds a nested tree from the daemon's flat file listing. Directory nodes are
 * inferred from path segments even when the listing only contains files, which
 * matches the sandbox `files/list` payload. Directories sort before files and
 * both are ordered alphabetically.
 */
export function buildFileTree(entries: ProjectFileEntry[]): FileTreeNode[] {
  const root: FileTreeNode = {
    name: "",
    path: "",
    isDir: true,
    children: [],
  };

  for (const entry of entries) {
    const segments = entry.path.split("/").filter((segment) => segment.length);
    if (segments.length === 0) {
      continue;
    }
    let cursor = root;
    segments.forEach((segment, index) => {
      const isLast = index === segments.length - 1;
      const isDir = isLast ? entry.is_dir : true;
      const path = segments.slice(0, index + 1).join("/");
      let child = cursor.children.find((node) => node.name === segment);
      if (!child) {
        child = { name: segment, path, isDir, children: [] };
        cursor.children.push(child);
      } else if (isDir) {
        child.isDir = true;
      }
      cursor = child;
    });
  }

  sortTree(root);
  return root.children;
}

function sortTree(node: FileTreeNode): void {
  node.children.sort((a, b) => {
    if (a.isDir !== b.isDir) {
      return a.isDir ? -1 : 1;
    }
    return a.name.localeCompare(b.name);
  });
  for (const child of node.children) {
    sortTree(child);
  }
}

type FileTreeProps = {
  entries: ProjectFileEntry[];
  activePath: string | undefined;
  onSelect: (path: string) => void;
  label: string;
};

export const FileTree = (props: FileTreeProps) => {
  return (
    <ul aria-label={props.label} class="jobs-file-tree">
      <For each={buildFileTree(props.entries)}>
        {(node) => (
          <FileTreeItem
            activePath={props.activePath}
            node={node}
            onSelect={props.onSelect}
          />
        )}
      </For>
    </ul>
  );
};

type FileTreeItemProps = {
  node: FileTreeNode;
  activePath: string | undefined;
  onSelect: (path: string) => void;
};

const FileTreeItem = (props: FileTreeItemProps) => {
  const [open, setOpen] = createSignal(false);

  return (
    <Show
      when={props.node.isDir}
      fallback={
        <li>
          <button
            aria-current={
              props.activePath === props.node.path ? "true" : undefined
            }
            class={
              props.activePath === props.node.path
                ? "jobs-file-tree__file jobs-file-tree__file--active"
                : "jobs-file-tree__file"
            }
            onClick={() => props.onSelect(props.node.path)}
            type="button"
          >
            {props.node.name}
          </button>
        </li>
      }
    >
      <li>
        <button
          aria-expanded={open()}
          class="jobs-file-tree__folder"
          onClick={() => setOpen((value) => !value)}
          type="button"
        >
          <span
            aria-hidden="true"
            class={
              open()
                ? "jobs-file-tree__twist jobs-file-tree__twist--open"
                : "jobs-file-tree__twist"
            }
          />
          {props.node.name}
        </button>
        <Show when={open()}>
          <ul class="jobs-file-tree__group">
            <For each={props.node.children}>
              {(child) => (
                <FileTreeItem
                  activePath={props.activePath}
                  node={child}
                  onSelect={props.onSelect}
                />
              )}
            </For>
          </ul>
        </Show>
      </li>
    </Show>
  );
};
