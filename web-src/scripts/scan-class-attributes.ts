/**
 * Shared scanner for the class-attribute lint scripts
 * (`check-classlist-length.ts`, `find-near-duplicate-classes.ts`).
 *
 * Both checks must see exactly the same attributes, so the file walk and the
 * attribute pattern live here. `Bun.Glob` is used rather than `node:fs`
 * `globSync`, which older Bun releases within the supported engine range lack.
 */
import { readFileSync } from "node:fs";
import path from "node:path";

/** TSX sources the checks cover, relative to the `web-src` workspace root. */
export const CLASS_SCAN_PATTERN = "axinite/src/**/*.tsx";

/**
 * Matches a static `class="..."` attribute, tolerating whitespace around `=`,
 * values that span lines, and empty values.
 */
const CLASS_ATTRIBUTE = /class\s*=\s*"([^"]*)"/gu;

export type ClassAttribute = {
  /** Source file, relative to the workspace root. */
  relativePath: string;
  /** The raw attribute value. */
  value: string;
  /** Whitespace-separated class tokens (empty for an empty attribute). */
  tokens: string[];
};

export type ClassScan = {
  fileCount: number;
  attributes: ClassAttribute[];
};

/** Collect every static class attribute under {@link CLASS_SCAN_PATTERN}. */
export function scanClassAttributes(root: string = process.cwd()): ClassScan {
  const files = [...new Bun.Glob(CLASS_SCAN_PATTERN).scanSync({ cwd: root })];
  const attributes: ClassAttribute[] = [];
  for (const relativePath of files) {
    const source = readFileSync(path.join(root, relativePath), "utf8");
    for (const match of source.matchAll(CLASS_ATTRIBUTE)) {
      const value = match[1] ?? "";
      const tokens = value.trim().split(/\s+/u).filter(Boolean);
      attributes.push({ relativePath, value, tokens });
    }
  }
  return { fileCount: files.length, attributes };
}
