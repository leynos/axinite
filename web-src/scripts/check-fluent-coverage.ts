#!/usr/bin/env bun
import fs from "node:fs";
import path from "node:path";
import process from "node:process";

import { parse } from "@fluent/syntax";

import {
  COMPLETE_LOCALE_CODES,
  DEFAULT_LOCALE,
} from "../axinite/src/lib/i18n/supported-locales";

const LOCALES_DIR = path.resolve(process.cwd(), "axinite/public/locales");

function readMessageKeys(locale: string): Set<string> {
  const filePath = path.join(LOCALES_DIR, locale, "common.ftl");
  const resource = parse(fs.readFileSync(filePath, "utf8"), {});

  return new Set(
    resource.body
      .filter((entry) => entry.type === "Message" || entry.type === "Term")
      .map((entry) => entry.id.name)
  );
}

function main(): void {
  const baseKeys = readMessageKeys(DEFAULT_LOCALE);
  let hasFailure = false;

  for (const locale of COMPLETE_LOCALE_CODES) {
    const filePath = path.join(LOCALES_DIR, locale, "common.ftl");

    if (!fs.existsSync(filePath)) {
      console.error(
        `[ftl-coverage] Missing locale file for ${locale}: ${filePath}`
      );
      hasFailure = true;
      continue;
    }

    const localeKeys = readMessageKeys(locale);
    const missing = [...baseKeys].filter((key) => !localeKeys.has(key));

    if (missing.length === 0) {
      continue;
    }

    hasFailure = true;
    console.error(
      `[ftl-coverage] ${locale} is marked complete but is missing ${missing.length} message(s)`
    );
    missing.forEach((key) => {
      console.error(`  - ${key}`);
    });
  }

  if (hasFailure) {
    process.exit(1);
    return;
  }

  console.log(
    `[ftl-coverage] Complete locales fully cover ${baseKeys.size} base messages`
  );
}

main();
