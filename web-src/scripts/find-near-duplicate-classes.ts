#!/usr/bin/env bun
/**
 * Lint script: fails when a `class` attribute in the SPA sources repeats a
 * token. It shares its scanner with `check-classlist-length.ts`.
 */

import { scanClassAttributes } from "./scan-class-attributes";

const { fileCount, attributes } = scanClassAttributes();
const failures = attributes
  .filter(({ tokens }) => new Set(tokens).size !== tokens.length)
  .map(
    ({ relativePath, value }) =>
      `${relativePath}: duplicate class token in "${value}"`
  );

if (failures.length > 0) {
  console.error("Near-duplicate class check failed:");
  failures.forEach((failure) => {
    console.error(`- ${failure}`);
  });
  process.exit(1);
}

console.log(`Near-duplicate class check passed for ${fileCount} files.`);
