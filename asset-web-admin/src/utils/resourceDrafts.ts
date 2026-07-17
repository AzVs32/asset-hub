import type { Resource, ResourceKindOption } from "../api/contracts";
import type { Draft, UploadDraft } from "../features/resourceWorkspace/models";

export function toDraft(resource: Resource): Draft {
  return {
    name: resource.name,
    directory: resource.directory,
    kind: resource.kind,
    status: resource.status,
    description: resource.metadata.summary.description ?? "",
    tags: resource.metadata.summary.tags.join(", "),
  };
}

export function metadataFromDraft(draft: Draft) {
  return buildMetadata(draft.description, draft.tags);
}

export function metadataFromUpload(draft: UploadDraft) {
  return buildMetadata(draft.description, draft.tags);
}

export function emptyCreateDraft(): Draft {
  return {
    name: "",
    directory: "",
    kind: "core:unknown",
    status: "active",
    description: "",
    tags: "",
  };
}

export function emptyUploadDraft(): UploadDraft {
  return {
    file: null,
    name: "",
    kind: "",
    directory: "uploads",
    description: "",
    tags: "",
  };
}

export function normalizeDraftKind<T extends { kind: string }>(
  draft: T,
  options: ResourceKindOption[],
): T {
  if (options.some((option) => option.kind === draft.kind)) {
    return draft;
  }

  return { ...draft, kind: options[0]?.kind ?? draft.kind };
}

export function kindOptionLabel(option: ResourceKindOption): string {
  return `${"— ".repeat(option.ancestors.length)}${option.label} (${option.kind})`;
}

export function kindOptionHint(option: ResourceKindOption | undefined): string {
  if (!option) return "";
  const content = option.supports_content ? "content" : "metadata only";
  const actions = option.actions.length ? ` / ${option.actions.map((action) => action.id).join(", ")}` : "";
  const detect = option.detect
    ? ` / detects ${[...option.detect.mime_types, ...option.detect.extensions].join(", ")}`
    : "";
  const hierarchy = option.parent ? ` / parent ${option.parent}` : " / root kind";
  return `${option.source}${hierarchy} / ${content}${detect}${actions}`;
}

export function sortKindsForHierarchy(options: ResourceKindOption[]): ResourceKindOption[] {
  const children = new Map<string | null, ResourceKindOption[]>();
  for (const option of options) {
    const parent = option.parent ?? null;
    const siblings = children.get(parent) ?? [];
    siblings.push(option);
    children.set(parent, siblings);
  }
  for (const siblings of children.values()) {
    siblings.sort((left, right) => left.label.localeCompare(right.label));
  }
  const sorted: ResourceKindOption[] = [];
  const visit = (parent: string | null) => {
    for (const child of children.get(parent) ?? []) {
      sorted.push(child);
      visit(child.kind);
    }
  };
  visit(null);
  return sorted;
}

export function directoriesFromResources(resources: Resource[]): string[] {
  const directories = new Set<string>(["uploads"]);
  for (const resource of resources) {
    const segments = resource.directory.split("/").filter(Boolean);
    for (let index = 0; index < segments.length; index += 1) {
      directories.add(segments.slice(0, index + 1).join("/"));
    }
  }

  return [...directories].sort((left, right) => left.localeCompare(right));
}

export function normalizeDirectoryInput(value: string): string {
  return value
    .trim()
    .replace(/\\/g, "/")
    .split("/")
    .map((part: string) => part.trim())
    .filter((part: string) => part && part !== ".")
    .join("/");
}

function buildMetadata(description: string, tags: string) {
  return {
    summary: {
      description: description.trim() || null,
      tags: splitTags(tags),
    },
  };
}

function splitTags(value: string): string[] {
  return value
    .split(",")
    .map((tag) => tag.trim())
    .filter(Boolean);
}

export function formatBytes(value: number): string {
  if (!value) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  const index = Math.min(Math.floor(Math.log(value) / Math.log(1024)), units.length - 1);
  const amount = value / 1024 ** index;
  return `${amount.toFixed(index === 0 ? 0 : 1)} ${units[index]}`;
}

export function formatDate(value: string): string {
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(value));
}

export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : "Request failed";
}
