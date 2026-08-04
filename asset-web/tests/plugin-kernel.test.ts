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
      ui: { group: "view", order: 20, locations: [hostSlots.resourceDetailActions] },
    });
    const earlier = action({
      id: "earlier",
      label: "Earlier",
      ui: { group: "view", order: 10, locations: [hostSlots.resourceDetailActions] },
    });
    expect(
      kernel
        .actionsAt(resource([later, earlier]), hostSlots.resourceDetailActions)
        .map((item) => item.id),
    ).toEqual(["earlier", "later"]);
  });

  it("keeps actions with unknown locations reachable in the detail action slot", () => {
    const kernel = new PluginKernel();
    const unknown = action({
      id: "future",
      ui: { group: null, order: null, locations: ["future_slot"] },
    });
    expect(kernel.actionsAt(resource([unknown]), hostSlots.resourceDetailActions)).toEqual([
      unknown,
    ]);
  });

  it("uses an explicit thumbnail action before media fallbacks", () => {
    const kernel = new PluginKernel();
    const fallback = action({ id: "media", output: { views: ["media"] } });
    const explicit = action({
      id: "azvs.epub.thumbnail",
      provides: "thumbnail",
      ui: { group: null, order: null, locations: [hostSlots.resourceListThumbnail] },
    });
    expect(kernel.thumbnailAction(resource([fallback, explicit]))?.id).toBe("azvs.epub.thumbnail");
  });

  it("does not infer a thumbnail action from MIME type or output view", () => {
    const kernel = new PluginKernel();
    const media = action({ id: "media", output: { views: ["media"] } });
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
      provides: "thumbnail",
      label: "Thumbnail",
      description: null,
      access: "read_only" as const,
      requires: { children: false, resources: false },
      output: { views: ["media" as const] },
      ui: {
        group: "preview",
        order: 100,
        locations: [hostSlots.directoryListThumbnail],
      },
      appliesTo: { kinds: ["core:directory"] },
    };
    const directory = {
      id: "directory-1",
      path: "books",
      parentPath: "",
      name: "books",
      kind: "core:directory",
      actions: [thumbnail],
    };

    expect(kernel.directoryThumbnailAction(directory)?.id).toBe("core.directory.thumbnail");
  });
});
