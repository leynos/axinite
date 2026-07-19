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

  if ((node as { type?: string }).type === "VariableReference") {
    const name = (node as { id?: { name?: string } }).id?.name;
    if (name) {
      vars.add(name);
    }
  }

  for (const value of Object.values(node)) {
    if (typeof value === "object" && value !== null) {
      collectVariableNames(value, vars);
    }
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

function main(): void {
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

  const baseMap = readLocalePlaceholders(BASE_LOCALE);
  const comparisonLocales = locales.filter((locale) => locale !== BASE_LOCALE);
  let hasMismatch = false;

  comparisonLocales.forEach((locale) => {
    const localeMap = readLocalePlaceholders(locale);

    baseMap.forEach((baseEntry, id) => {
      if (baseEntry.ignore) {
        return;
      }

      const target = localeMap.get(id);
      if (!target || target.ignore) {
        return;
      }

      const missing = [...baseEntry.vars].filter(
        (name) => !target.vars.has(name)
      );
      const extra = [...target.vars].filter(
        (name) => !baseEntry.vars.has(name)
      );

      if (missing.length === 0 && extra.length === 0) {
        return;
      }

      hasMismatch = true;
      console.error(
        `[ftl-vars] ${locale}:${id} placeholder mismatch vs ${BASE_LOCALE}`
      );
      if (missing.length > 0) {
        console.error(
          `  missing: ${missing.map((name) => `$${name}`).join(", ")}`
        );
      }
      if (extra.length > 0) {
        console.error(
          `  extra:   ${extra.map((name) => `$${name}`).join(", ")}`
        );
      }
    });
  });

  if (hasMismatch || process.exitCode) {
    console.error("[ftl-vars] Placeholder validation failed");
    process.exit(1);
    return;
  }

  console.log("[ftl-vars] All Fluent placeholders align across locales");
}

main();
