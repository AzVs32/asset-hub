import type { Directory, DirectoryAction, Resource, ResourceAction } from "@/domain/resource";

export function action(
  input: Omit<Partial<ResourceAction>, "ui"> &
    Pick<ResourceAction, "id"> & { ui?: Partial<ResourceAction["ui"]> },
): ResourceAction {
  return {
    id: input.id,
    origin: input.origin ?? { kind: "builtin", id: "test" },
    provides: input.provides ?? null,
    label: input.label ?? input.id,
    description: input.description ?? null,
    access: input.access ?? "read",
    requires: input.requires ?? { content: false, contentDelivery: "auto" },
    output: input.output ?? { views: ["json"], effects: [] },
    ui: {
      group: input.ui?.group ?? null,
      order: input.ui?.order ?? null,
      locations: input.ui?.locations ?? [],
      destructive: input.ui?.destructive ?? false,
      confirmation: input.ui?.confirmation ?? null,
    },
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

export function directoryAction(
  input: Omit<Partial<DirectoryAction>, "ui"> &
    Pick<DirectoryAction, "id"> & { ui?: Partial<DirectoryAction["ui"]> },
): DirectoryAction {
  return {
    id: input.id,
    origin: input.origin ?? { kind: "plugin", id: "test.plugin" },
    provides: input.provides ?? null,
    label: input.label ?? input.id,
    description: input.description ?? null,
    access: input.access ?? "read",
    requires: input.requires ?? { children: false, resources: false },
    output: input.output ?? { views: ["json"], effects: [] },
    ui: {
      group: input.ui?.group ?? null,
      order: input.ui?.order ?? null,
      locations: input.ui?.locations ?? [],
      destructive: input.ui?.destructive ?? false,
      confirmation: input.ui?.confirmation ?? null,
    },
    appliesTo: input.appliesTo ?? { kinds: [] },
  };
}

export function directory(actions: DirectoryAction[] = []): Directory {
  return {
    id: "directory-1",
    parentId: null,
    path: "library",
    parentPath: "",
    name: "Library",
    kind: "core:directory",
    actions,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
    revision: 1,
  };
}
