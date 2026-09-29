#!/usr/bin/env bun
/**
 * Lint script: fails when a `class` attribute in the SPA sources has more
 * than 12 tokens, which usually signals that styles belong in a semantic
 * class. It shares its scanner with `find-near-duplicate-classes.ts`.
 */

import { scanClassAttributes } from "./scan-class-attributes";

const MAX_CLASS_TOKENS = 12;
const { fileCount, attributes } = scanClassAttributes();
const failures = attributes
  .filter(({ tokens }) => tokens.length > MAX_CLASS_TOKENS)
  .map(
    ({ relativePath, tokens }) =>
      `${relativePath}: class attribute contains ${tokens.length} tokens`
  );

if (failures.length > 0) {
  console.error("Class list length check failed:");
  failures.forEach((failure) => {
    console.error(`- ${failure}`);
  });
  process.exit(1);
}

console.log(`Class list length check passed for ${fileCount} files.`);
