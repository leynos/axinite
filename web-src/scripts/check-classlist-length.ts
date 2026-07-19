#!/usr/bin/env bun
import { globSync, readFileSync } from "node:fs";
import path from "node:path";

const MAX_CLASS_TOKENS = 12;
const root = process.cwd();
const files = globSync("axinite/src/**/*.tsx", { cwd: root });
const failures: string[] = [];

for (const relativePath of files) {
  const absolutePath = path.join(root, relativePath);
  const source = readFileSync(absolutePath, "utf8");

  for (const match of source.matchAll(/class="([^"\n]+)"/g)) {
    const value = match[1] ?? "";
    const tokens = value.trim().split(/\s+/u).filter(Boolean);

    if (tokens.length > MAX_CLASS_TOKENS) {
      failures.push(
        `${relativePath}: class attribute contains ${tokens.length} tokens`
      );
    }
  }
}

if (failures.length > 0) {
  console.error("Class list length check failed:");
  failures.forEach((failure) => {
    console.error(`- ${failure}`);
  });
  process.exit(1);
}

console.log(`Class list length check passed for ${files.length} files.`);
