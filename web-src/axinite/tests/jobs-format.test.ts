import { describe, expect, it } from "vitest";

import { resolveActiveFilePath } from "@/components/jobs/format";
import type { ProjectFileEntry } from "@/lib/api/contracts";

const entries: ProjectFileEntry[] = [
  { name: "notes", path: "notes", is_dir: true },
  { name: "a.md", path: "a.md", is_dir: false },
  { name: "b.md", path: "b.md", is_dir: false },
];

describe("resolveActiveFilePath", () => {
  it("keeps the operator's selection while it is still listed", () => {
    expect(resolveActiveFilePath(entries, "b.md")).toBe("b.md");
  });

  it("defaults to the first file when nothing is selected", () => {
    expect(resolveActiveFilePath(entries, undefined)).toBe("a.md");
  });

  it("falls back to the first file when the selection disappears", () => {
    expect(resolveActiveFilePath(entries, "gone.md")).toBe("a.md");
  });

  it("selects nothing when the listing has no files", () => {
    expect(
      resolveActiveFilePath([{ name: "d", path: "d", is_dir: true }], "x")
    ).toBeUndefined();
  });
});
