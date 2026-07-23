import { describe, expect, it } from "vitest";
import { breadcrumbs, normalizeDirectory, parentDirectory } from "@/domain/directory-path";

describe("resource directory navigation", () => {
  it("normalizes separators and empty segments without changing spaces", () => {
    expect(normalizeDirectory("/ library /project  A/./drafts /")).toBe(
      " library /project  A/drafts ",
    );
  });

  it("does not navigate above the visible root", () => {
    expect(parentDirectory("project/raw")).toBe("project");
    expect(parentDirectory("")).toBeNull();
  });

  it("builds breadcrumbs from visible paths only", () => {
    expect(breadcrumbs("project/raw")).toEqual([
      { path: "", label: "Root" },
      { path: "project", label: "project" },
      { path: "project/raw", label: "raw" },
    ]);
  });
});
