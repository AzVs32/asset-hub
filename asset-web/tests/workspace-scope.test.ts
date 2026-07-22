import { describe, expect, it } from "vitest";
import { createWorkspaceScope } from "@/application/workspace/workspace-scope";
import { visibleDirectory } from "@/domain/directory-path";
import type { DirectoryListing, Resource } from "@/domain/resource";
import { resource as resourceFixture } from "./fixtures";

const member = {
  id: "azvs-1",
  username: "azvs",
  role: "member" as const,
  workspaceDirectory: "users/azvs",
  isAdmin: false,
};

describe("WorkspaceScope", () => {
  it("keeps storage paths out of member-facing listings", () => {
    const scope = createWorkspaceScope(member);
    const resource: Resource = {
      ...resourceFixture(),
      directory: "users/azvs/images",
    };
    const listing: DirectoryListing = {
      path: "users/azvs/images",
      folders: [
        {
          path: "users/azvs/images/raw",
          parentPath: "users/azvs/images",
          name: "raw",
        },
      ],
      resources: { items: [resource], total: 1, page: 1, limit: 30 },
    };

    expect(scope.toVisibleListing(listing)).toMatchObject({
      path: "images",
      folders: [{ path: "images/raw", parentPath: "images" }],
      resources: { items: [{ directory: "images" }] },
    });
  });

  it("adds the storage root only at command and query boundaries", () => {
    const scope = createWorkspaceScope(member);
    expect(
      scope.toStorageFilters({
        directory: visibleDirectory("images"),
        page: 2,
        limit: 30,
        query: "photo",
        tag: "",
        kind: "",
        includeDescendants: false,
        includeDeleted: false,
      }).directory,
    ).toBe("users/azvs/images");
    expect(
      scope.toStorageResourceDraft({
        name: "cover",
        directory: "/",
        kind: "image",
        status: "active",
        description: "",
        tags: "",
      }).directory,
    ).toBe("users/azvs");
  });

  it("rejects storage data outside the current member workspace", () => {
    const scope = createWorkspaceScope(member);
    expect(() =>
      scope.toVisibleResource({ ...resourceFixture(), directory: "users/alice/private" }),
    ).toThrow("outside the current workspace");
  });

  it("uses an identity mapping for administrators", () => {
    const scope = createWorkspaceScope({
      id: "admin-1",
      username: "admin",
      role: "administrator",
      workspaceDirectory: "",
      isAdmin: true,
    });
    expect(scope.toStorageDirectory("library/images")).toBe("library/images");
    expect(scope.toVisibleDirectory("library/images")).toBe("library/images");
  });
});
