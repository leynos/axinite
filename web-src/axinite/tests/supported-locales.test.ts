import { describe, expect, it } from "vitest";

import {
  AVAILABLE_LOCALES,
  COMPLETE_LOCALE_CODES,
  DEFAULT_LOCALE,
  getLocaleDirection,
  getLocaleMetadata,
} from "@/lib/i18n/supported-locales";

describe("supported locales", () => {
  it("falls back to the default locale when the code is unknown", () => {
    expect(getLocaleMetadata("unknown-tag").code).toBe(DEFAULT_LOCALE);
  });

  it("maps Arabic to rtl", () => {
    expect(getLocaleDirection("ar")).toBe("rtl");
  });

  it("maps French to ltr", () => {
    expect(getLocaleDirection("fr")).toBe("ltr");
  });

  it("only exposes locales marked complete in the picker/runtime list", () => {
    expect(AVAILABLE_LOCALES.map((locale) => locale.code)).toEqual([
      ...COMPLETE_LOCALE_CODES,
    ]);
  });
});
