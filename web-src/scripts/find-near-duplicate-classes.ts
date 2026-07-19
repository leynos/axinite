#!/usr/bin/env bun
import { globSync, readFileSync } from "node:fs";
import path from "node:path";

const root = process.cwd();
const files = globSync("axinite/src/**/*.tsx", { cwd: root });
const failures: string[] = [];

for (const relativePath of files) {
  const absolutePath = path.join(root, relativePath);
  const source = readFileSync(absolutePath, "utf8");

  for (const match of source.matchAll(/class\s*=\s*"([^"]*)"/gs)) {
    const value = match[1] ?? "";
    const tokens = value.trim().split(/\s+/u).filter(Boolean);
    const uniqueTokens = new Set(tokens);
    if (uniqueTokens.size !== tokens.length) {
      failures.push(`${relativePath}: duplicate class token in "${value}"`);
    }
  }
}

if (failures.length > 0) {
  console.error("Near-duplicate class check failed:");
  failures.forEach((failure) => {
    console.error(`- ${failure}`);
  });
  process.exit(1);
}

console.log(`Near-duplicate class check passed for ${files.length} files.`);
