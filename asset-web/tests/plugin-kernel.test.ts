import { describe, expect, it } from "vitest";
import { PluginKernel } from "@/kernel/plugin-kernel";
import { coreDirectoryWorkspaceSlots, directoryWorkspaceOutlet } from "@/kernel/slots";
import { action, directory, directoryAction, resource } from "./fixtures";

describe("PluginKernel", () => {
  it("keeps unknown Resource locations reachable only in the Core context menu", () => {
    const kernel = new PluginKernel();
    const unknown = action({
      id: "future",
      ui: { group: null, order: null, locations: ["future_slot"] },
    });

    expect(
      kernel.resourceActionsAtCoreSlot(
        resource([unknown]),
        coreDirectoryWorkspaceSlots.resourceContextMenu,
      ),
    ).toEqual([unknown]);
  });

  it("uses only the explicit read-only Resource thumbnail provider", () => {
    const kernel = new PluginKernel();
    const fallback = action({ id: "media", output: { views: ["media"], effects: [] } });
    const explicit = action({
      id: "azvs.epub.thumbnail",
      provides: "thumbnail",
      ui: {
        group: null,
        order: null,
        locations: [coreDirectoryWorkspaceSlots.resourceThumbnail],
      },
    });

    expect(kernel.thumbnailAction(resource([fallback, explicit]))?.id).toBe("azvs.epub.thumbnail");
  });

  it("selects Core Directory thumbnail and context-menu actions", () => {
    const kernel = new PluginKernel();
    const thumbnail = directoryAction({
      id: "core.directory.thumbnail",
      origin: { kind: "builtin", id: "core.directory" },
      provides: "thumbnail",
      output: { views: ["media"], effects: [] },
      ui: { locations: [coreDirectoryWorkspaceSlots.directoryThumbnail] },
    });
    const unknown = directoryAction({
      id: "example.organize",
      ui: { locations: ["future_directory_slot"] },
    });
    const item = directory([thumbnail, unknown]);

    expect(kernel.directoryThumbnailAction(item)?.id).toBe("core.directory.thumbnail");
    expect(
      kernel
        .directoryActionsAtCoreSlot(item, coreDirectoryWorkspaceSlots.directoryContextMenu)
        .map((action) => action.id),
    ).toEqual(["example.organize"]);
  });

  it("hands the entire Directory workspace to one valid workspace provider", () => {
    const kernel = new PluginKernel();
    const workspace = directoryAction({
      id: "example.game.workspace",
      provides: "workspace",
      output: { views: ["plugin_frame", "json"], effects: [] },
      ui: { locations: [directoryWorkspaceOutlet] },
    });
    const item = directory([workspace]);

    expect(kernel.directoryWorkspaceAction(item)).toBe(workspace);
    expect(
      kernel.directoryActionsAtCoreSlot(item, coreDirectoryWorkspaceSlots.directoryContextMenu),
    ).toEqual([]);
  });
});
