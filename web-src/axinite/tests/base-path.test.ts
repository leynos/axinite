import { describe, expect, it } from "vitest";
import {
  buildAppPath,
  GITHUB_PAGES_BASE_PATH,
  normaliseBasePath,
} from "@/lib/base-path";
import { buildFluentLoadPath } from "@/lib/i18n/runtime";

describe("base path helpers", () => {
  it("normalises the GitHub Pages base path", () => {
    expect(normaliseBasePath(GITHUB_PAGES_BASE_PATH)).toBe("/axinite-mockup/");
  });

  it("builds route paths under the deploy prefix", () => {
    expect(buildAppPath(GITHUB_PAGES_BASE_PATH, "/chat")).toBe(
      "/axinite-mockup/chat"
    );
    expect(buildAppPath(GITHUB_PAGES_BASE_PATH, "skills")).toBe(
      "/axinite-mockup/skills"
    );
  });

  it("builds locale asset paths under the same deploy prefix", () => {
    expect(buildFluentLoadPath(GITHUB_PAGES_BASE_PATH)).toBe(
      "/axinite-mockup/locales/{{lng}}/{{ns}}.ftl"
    );
  });
});
