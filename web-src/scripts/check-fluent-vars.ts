#!/usr/bin/env bun
import fs from "node:fs";
import path from "node:path";
import process from "node:process";

import { parse, type Resource } from "@fluent/syntax";

type PlaceholderReport = {
  vars: Set<string>;
  ignore: boolean;
};

const BASE_LOCALE = process.env.FTL_BASE_LOCALE ?? "en-GB";
const LOCALES_DIR = path.resolve(process.cwd(), "axinite/public/locales");
const IGNORE_MARKER = "vars: ignore-mismatch";

function listLocaleDirs(): string[] {
  return fs
    .readdirSync(LOCALES_DIR, { withFileTypes: true })
    .filter((dirent) => dirent.isDirectory())
    .map((dirent) => dirent.name);
}

function listFtlFiles(locale: string): string[] {
  const startDir = path.join(LOCALES_DIR, locale);
  const queue = [startDir];
  const files: string[] = [];

  while (queue.length > 0) {
    const current = queue.pop();
    if (!current) {
      continue;
    }

    const stats = fs.statSync(current);
    if (stats.isDirectory()) {
      const children = fs.readdirSync(current);
      children.forEach((child) => {
        queue.push(path.join(current, child));
      });
      continue;
    }

    if (stats.isFile() && current.endsWith(".ftl")) {
      files.push(current);
    }
  }

  return files;
}

function variableReferenceName(node: object): string | undefined {
  if ((node as { type?: string }).type !== "VariableReference") {
    return undefined;
  }
  return (node as { id?: { name?: string } }).id?.name;
}

function collectVariableNames(node: unknown, vars: Set<string>): void {
  if (!node || typeof node !== "object") {
    return;
  }

  if (Array.isArray(node)) {
    node.forEach((child) => {
      collectVariableNames(child, vars);
    });
    return;
  }

  const name = variableReferenceName(node);
  if (name) {
    vars.add(name);
  }

  // Non-object values are ignored by the guard above, so recursing into every
  // property value is equivalent to filtering for objects here.
  for (const value of Object.values(node)) {
    collectVariableNames(value, vars);
  }
}

function parseResource(
  contents: string,
  filePath: string
): Resource | undefined {
  try {
    return parse(contents, {});
  } catch (error) {
    console.error(
      `[ftl-vars] Unable to parse ${filePath}:`,
      (error as Error).message
    );
    process.exitCode = 1;
    return undefined;
  }
}

function shouldIgnoreEntry(entry: {
  comment?: { content?: string } | null;
}): boolean {
  const comment = entry.comment?.content ?? "";
  return comment.includes(IGNORE_MARKER);
}

function readLocalePlaceholders(
  locale: string
): Map<string, PlaceholderReport> {
  const files = listFtlFiles(locale);
  const results = new Map<string, PlaceholderReport>();

  files.forEach((filePath) => {
    let contents: string;

    try {
      contents = fs.readFileSync(filePath, "utf8");
    } catch (error) {
      console.error(
        `[ftl-vars] Unable to read ${filePath}:`,
        (error as Error).message
      );
      process.exitCode = 1;
      return;
    }

    const resource = parseResource(contents, filePath);
    if (!resource) {
      return;
    }

    resource.body.forEach((entry) => {
      if (entry.type !== "Message" && entry.type !== "Term") {
        return;
      }

      const id = entry.id?.name;
      if (!id) {
        return;
      }

      const vars = results.get(id)?.vars ?? new Set<string>();
      collectVariableNames(entry.value, vars);
      entry.attributes?.forEach((attribute) => {
        collectVariableNames(attribute.value, vars);
      });

      const ignore =
        shouldIgnoreEntry(entry) || results.get(id)?.ignore === true;
      results.set(id, { vars, ignore });
    });
  });

  return results;
}

type PlaceholderDiff = { missing: string[]; extra: string[] };

// Resolve the locale directories, exiting with a diagnostic when the locales
// directory or the base locale is absent.
function resolveLocales(): string[] {
  if (!fs.existsSync(LOCALES_DIR)) {
    console.error(`[ftl-vars] No locales directory at ${LOCALES_DIR}`);
    process.exit(1);
  }

  const locales = listLocaleDirs();
  if (!locales.includes(BASE_LOCALE)) {
    console.error(
      `[ftl-vars] Base locale "${BASE_LOCALE}" not found under ${LOCALES_DIR}`
    );
    process.exit(1);
  }

  return locales;
}

// Compare a single entry against its base counterpart, returning the
// placeholder differences or null when the entry is ignored or aligned.
function diffPlaceholders(
  baseEntry: PlaceholderReport,
  target: PlaceholderReport | undefined
): PlaceholderDiff | null {
  if (baseEntry.ignore || !target || target.ignore) {
    return null;
  }

  const missing = [...baseEntry.vars].filter((name) => !target.vars.has(name));
  const extra = [...target.vars].filter((name) => !baseEntry.vars.has(name));

  if (missing.length === 0 && extra.length === 0) {
    return null;
  }

  return { missing, extra };
}

function reportMismatch(
  locale: string,
  id: string,
  diff: PlaceholderDiff
): void {
  console.error(
    `[ftl-vars] ${locale}:${id} placeholder mismatch vs ${BASE_LOCALE}`
  );
  if (diff.missing.length > 0) {
    console.error(
      `  missing: ${diff.missing.map((name) => `$${name}`).join(", ")}`
    );
  }
  if (diff.extra.length > 0) {
    console.error(
      `  extra:   ${diff.extra.map((name) => `$${name}`).join(", ")}`
    );
  }
}

// Report every placeholder mismatch in one locale, returning whether any were
// found.
function compareLocale(
  baseMap: Map<string, PlaceholderReport>,
  locale: string
): boolean {
  const localeMap = readLocalePlaceholders(locale);
  let hasMismatch = false;

  baseMap.forEach((baseEntry, id) => {
    const diff = diffPlaceholders(baseEntry, localeMap.get(id));
    if (!diff) {
      return;
    }
    hasMismatch = true;
    reportMismatch(locale, id, diff);
  });

  return hasMismatch;
}

function main(): void {
  const locales = resolveLocales();
  const baseMap = readLocalePlaceholders(BASE_LOCALE);
  const comparisonLocales = locales.filter((locale) => locale !== BASE_LOCALE);

  let hasMismatch = false;
  for (const locale of comparisonLocales) {
    if (compareLocale(baseMap, locale)) {
      hasMismatch = true;
    }
  }

  if (hasMismatch || process.exitCode) {
    console.error("[ftl-vars] Placeholder validation failed");
    process.exit(1);
  }

  console.log("[ftl-vars] All Fluent placeholders align across locales");
}

main();
