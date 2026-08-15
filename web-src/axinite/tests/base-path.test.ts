import { describe, expect, it } from "vitest";
import {
  buildAppPath,
  DEPLOY_BASE_PATH,
  normalizeBasePath,
} from "@/lib/base-path";
import { buildFluentLoadPath } from "@/lib/i18n/runtime";

describe("base path helpers", () => {
  it("normalizes the deploy base path", () => {
    expect(normalizeBasePath(DEPLOY_BASE_PATH)).toBe("/");
  });

  it("normalizes a prefixed base path", () => {
    expect(normalizeBasePath("/preview")).toBe("/preview/");
  });

  it("builds route paths under the deploy base", () => {
    expect(buildAppPath(DEPLOY_BASE_PATH, "/chat")).toBe("/chat");
    expect(buildAppPath(DEPLOY_BASE_PATH, "skills")).toBe("/skills");
  });

  it("builds locale asset paths under the same deploy base", () => {
    expect(buildFluentLoadPath(DEPLOY_BASE_PATH)).toBe(
      "/locales/{{lng}}/{{ns}}.ftl"
    );
  });
});
