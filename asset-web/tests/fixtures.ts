import type { Resource, ResourceAction } from "@/domain/resource";

export function action(
  input: Partial<ResourceAction> & Pick<ResourceAction, "id">,
): ResourceAction {
  return {
    id: input.id,
    label: input.label ?? input.id,
    description: input.description ?? null,
    access: input.access ?? "read_only",
    executor: input.executor ?? { type: "plugin", handler: "run" },
    requires: input.requires ?? { content: false, contentDelivery: "auto" },
    output: input.output ?? { views: ["json"] },
    ui: input.ui ?? { group: null, order: null, locations: [] },
    appliesTo: input.appliesTo ?? { kinds: [], mimeTypes: [], extensions: [] },
  };
}

export function resource(actions: ResourceAction[] = []): Resource {
  return {
    id: "resource-1",
    name: "Example",
    directory: "library",
    kind: "core:video",
    status: "active",
    metadata: { summary: { description: null, tags: [] }, kindMetadata: null },
    content: {
      key: "library/example.mp4",
      size: 128,
      mimeType: "video/mp4",
      originalFilename: "example.mp4",
      checksums: [],
    },
    actions,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
    deletedAt: null,
  };
}
