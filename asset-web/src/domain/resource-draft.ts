import type { Resource, ResourceDraft } from "./resource";

export function draftFromResource(resource: Resource): ResourceDraft {
  return {
    name: resource.name,
    directory: resource.directory,
    kind: resource.kind,
  };
}
