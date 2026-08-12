import { describe, expect, it } from "vitest";
import { PluginKernel } from "@/kernel/plugin-kernel";
import { hostSlots } from "@/kernel/slots";
import { action, resource } from "./fixtures";

describe("PluginKernel", () => {
  it("sorts actions placed in a stable host slot", () => {
    const kernel = new PluginKernel();
    const later = action({
      id: "later",
      label: "Later",
      ui: { group: "view", order: 20, locations: [hostSlots.resourceContextMenu] },
    });
    const earlier = action({
      id: "earlier",
      label: "Earlier",
      ui: { group: "view", order: 10, locations: [hostSlots.resourceContextMenu] },
    });
    expect(
      kernel
        .actionsAt(resource([later, earlier]), hostSlots.resourceContextMenu)
        .map((item) => item.id),
    ).toEqual(["earlier", "later"]);
  });

  it("sorts destructive actions after ordinary actions", () => {
    const kernel = new PluginKernel();
    const remove = action({
      id: "core.resource.delete",
      access: "write",
      output: { views: [], effects: ["delete"] },
      ui: {
        group: "danger",
        locations: [hostSlots.resourceContextMenu],
        destructive: true,
      },
    });
    const download = action({
      id: "core.resource.download",
      ui: { group: "open", locations: [hostSlots.resourceContextMenu] },
    });

    expect(
      kernel
        .actionsAt(resource([remove, download]), hostSlots.resourceContextMenu)
        .map((item) => item.id),
    ).toEqual(["core.resource.download", "core.resource.delete"]);
  });

  it("keeps actions with unknown locations reachable in the resource context menu", () => {
    const kernel = new PluginKernel();
    const unknown = action({
      id: "future",
      ui: { group: null, order: null, locations: ["future_slot"] },
    });
    expect(kernel.actionsAt(resource([unknown]), hostSlots.resourceContextMenu)).toEqual([unknown]);
  });

  it("uses an explicit thumbnail action before media fallbacks", () => {
    const kernel = new PluginKernel();
    const fallback = action({ id: "media", output: { views: ["media"], effects: [] } });
    const explicit = action({
      id: "azvs.epub.thumbnail",
      provides: "thumbnail",
      ui: { group: null, order: null, locations: [hostSlots.resourceThumbnail] },
    });
    expect(kernel.thumbnailAction(resource([fallback, explicit]))?.id).toBe("azvs.epub.thumbnail");
  });

  it("does not infer a thumbnail action from MIME type or output view", () => {
    const kernel = new PluginKernel();
    const media = action({ id: "media", output: { views: ["media"], effects: [] } });
    const item = resource([media]);
    item.content = {
      size: 42,
      mimeType: "image/png",
      verificationStatus: "verified",
      checksum: { kind: "sha256", value: "digest" },
      verificationError: null,
    };
    expect(kernel.thumbnailAction(item)).toBeNull();
  });

  it("selects a read-only directory thumbnail provider", () => {
    const kernel = new PluginKernel();
    const thumbnail = {
      id: "core.directory.thumbnail",
      origin: { kind: "builtin" as const, id: "core.directory" },
      provides: "thumbnail",
      label: "Thumbnail",
      description: null,
      access: "read" as const,
      requires: { children: false, resources: false },
      output: { views: ["media" as const], effects: [] },
      ui: {
        group: "preview",
        order: 100,
        locations: [hostSlots.directoryThumbnail],
        destructive: false,
        confirmation: null,
      },
      appliesTo: { kinds: ["core:directory"] },
    };
    const directory = {
      id: "directory-1",
      parentId: null,
      path: "books",
      parentPath: "",
      name: "books",
      kind: "core:directory",
      actions: [thumbnail],
      createdAt: "2026-01-01T00:00:00Z",
      updatedAt: "2026-01-01T00:00:00Z",
      revision: 1,
    };

    expect(kernel.directoryThumbnailAction(directory)?.id).toBe("core.directory.thumbnail");
  });

  it("keeps directory actions with unknown locations reachable in the context menu", () => {
    const kernel = new PluginKernel();
    const directory = {
      id: "directory-1",
      parentId: null,
      path: "books",
      parentPath: "",
      name: "books",
      kind: "core:directory",
      actions: [
        {
          id: "example.organize",
          origin: { kind: "plugin" as const, id: "example.plugin" },
          provides: null,
          label: "Organize",
          description: null,
          access: "read" as const,
          requires: { children: false, resources: false },
          output: { views: ["json" as const], effects: [] },
          ui: {
            group: null,
            order: null,
            locations: ["future_directory_slot"],
            destructive: false,
            confirmation: null,
          },
          appliesTo: { kinds: ["core:directory"] },
        },
      ],
      createdAt: "2026-01-01T00:00:00Z",
      updatedAt: "2026-01-01T00:00:00Z",
      revision: 1,
    };

    expect(
      kernel.directoryActionsAt(directory, hostSlots.directoryContextMenu).map((item) => item.id),
    ).toEqual(["example.organize"]);
  });
});
