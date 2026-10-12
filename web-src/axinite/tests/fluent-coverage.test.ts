/** @file Regression contracts for distinct Fluent values, terms and attributes. */

import { describe, expect, it } from "vitest";

import { fluentCoverageKeys } from "../../scripts/fluent-coverage";

describe("Fluent locale coverage", () => {
  it("does not let a message cover a term with the same identifier", () => {
    const base = fluentCoverageKeys("foo = Message\n-foo = Term\n");
    const translated = fluentCoverageKeys("foo = Translation\n");
    expect([...base].filter((key) => !translated.has(key))).toEqual(["-foo"]);
  });

  it("requires each attribute independently of its value and other attributes", () => {
    const base = fluentCoverageKeys(
      "foo = Value\n    .label = Label\n    .title = Title\n"
    );
    const translated = fluentCoverageKeys(
      "foo = Translation\n    .title = Title\n"
    );
    expect([...base].filter((key) => !translated.has(key))).toEqual([
      "foo.label",
    ]);
  });

  it("keeps term attributes distinct and does not invent attribute-only values", () => {
    expect([
      ...fluentCoverageKeys(
        "foo =\n    .label = Label\n-foo = Term\n    .label = Term label\n"
      ),
    ]).toEqual(["foo.label", "-foo", "-foo.label"]);
  });
});
