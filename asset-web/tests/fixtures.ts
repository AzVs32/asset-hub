import type { Resource, ResourceAction } from "@/domain/resource";

export function action(
  input: Partial<ResourceAction> & Pick<ResourceAction, "id">,
): ResourceAction {
  return {
    id: input.id,
    origin: input.origin ?? { kind: "builtin", id: "test" },
    provides: input.provides ?? null,
    label: input.label ?? input.id,
    description: input.description ?? null,
    access: input.access ?? "read",
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
    content: {
      size: 128,
      mimeType: "video/mp4",
      verificationStatus: "verified",
      checksum: {
        kind: "sha256",
        value: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
      },
      verificationError: null,
    },
    actions,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
    revision: 1,
    deletedAt: null,
  };
}
