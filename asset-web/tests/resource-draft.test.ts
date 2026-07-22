import { describe, expect, it } from "vitest";
import { createWorkspaceScope } from "@/application/workspace/workspace-scope";
import {
  breadcrumbs,
  normalizeDirectory,
  parentDirectory,
  visibleDirectory,
} from "@/domain/directory-path";

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

  it("maps paths through a session workspace scope", () => {
    const scope = createWorkspaceScope({
      id: "azvs-1",
      username: "azvs",
      role: "member",
      workspaceDirectory: "users/azvs",
      isAdmin: false,
    });
    expect(scope.toVisibleDirectory("users/azvs")).toBe("");
    expect(scope.toVisibleDirectory("users/azvs/images")).toBe("images");
    expect(scope.toStorageDirectory(visibleDirectory())).toBe("users/azvs");
    expect(scope.toStorageDirectory(visibleDirectory("images"))).toBe("users/azvs/images");
    expect(() => scope.toVisibleDirectory("users/alice")).toThrow("outside");
  });
});
