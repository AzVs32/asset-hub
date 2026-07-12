import type { Draft, Resource, ResourceActionDefinition, ResourceKindOption, UploadDraft } from "../types";

export function toDraft(resource: Resource): Draft {
  return {
    name: resource.name,
    directory: resource.directory,
    kind: resource.kind,
    status: resource.status,
    description: resource.metadata.summary.description ?? "",
    tags: resource.metadata.summary.tags.join(", "),
    schemaId: resource.metadata.kind?.schema_id ?? "",
    kindData: JSON.stringify(resource.metadata.kind?.data ?? {}, null, 2),
  };
}

export function metadataFromDraft(draft: Draft) {
  return buildMetadata(draft.description, draft.tags, draft.schemaId, draft.kindData);
}

export function metadataFromUpload(draft: UploadDraft) {
  return buildMetadata(draft.description, draft.tags, draft.schemaId, draft.kindData);
}

export function emptyCreateDraft(): Draft {
  return {
    name: "",
    directory: "",
    kind: "core:unknown",
    status: "active",
    description: "",
    tags: "",
    schemaId: "",
    kindData: "{}",
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
    schemaId: "",
    kindData: "{}",
  };
}

export function normalizeDraftKind<T extends { kind: string; schemaId: string }>(
  draft: T,
  options: ResourceKindOption[],
): T {
  if (options.some((option) => option.kind === draft.kind)) {
    return withKindDefaults(draft, options);
  }

  return withKindDefaults({ ...draft, kind: options[0]?.kind ?? draft.kind }, options);
}

export function withKindDefaults<T extends { kind: string; schemaId: string }>(
  draft: T,
  options: ResourceKindOption[],
): T {
  const option = options.find((item) => item.kind === draft.kind);
  if (!option) return draft;

  return {
    ...draft,
    schemaId: option.schema_id ?? "",
  };
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
  return `${option.source}${hierarchy} / ${content}${option.schema_id ? ` / ${option.schema_id}` : ""}${detect}${actions}`;
}

export function sortKindsForHierarchy(options: ResourceKindOption[]): ResourceKindOption[] {
  const children = new Map<string | null, ResourceKindOption[]>();
  for (const option of options) {
    const siblings = children.get(option.parent) ?? [];
    siblings.push(option);
    children.set(option.parent, siblings);
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

export function isImageResource(resource: Resource): boolean {
  const mimeType = resource.content?.mime_type;
  return Boolean(mimeType && mimeType.startsWith("image/"));
}

export function isPluginUiAction(action: ResourceActionDefinition): boolean {
  return action.executor.type === "plugin";
}

export function hasAction(resource: Resource, actionId: string): boolean {
  return resource.actions.available_actions.some((action) => action.id === actionId);
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

function buildMetadata(description: string, tags: string, schemaId: string, kindData: string) {
  const trimmedSchemaId = schemaId.trim();
  return {
    summary: {
      description: description.trim() || null,
      tags: splitTags(tags),
    },
    kind: trimmedSchemaId
      ? {
          schema_id: trimmedSchemaId,
          data: parseKindData(kindData),
        }
      : null,
  };
}

function splitTags(value: string): string[] {
  return value
    .split(",")
    .map((tag) => tag.trim())
    .filter(Boolean);
}

function parseKindData(value: string): Record<string, unknown> {
  const trimmed = value.trim();
  if (!trimmed) return {};

  const parsed = JSON.parse(trimmed);
  if (!parsed || Array.isArray(parsed) || typeof parsed !== "object") {
    throw new Error("Kind data must be a JSON object");
  }

  return parsed as Record<string, unknown>;
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
