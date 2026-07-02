import type { Draft, Resource, ResourceActionDefinition, ResourceKindOption, ResourceTreeRow, UploadDraft } from "../types";

export function toDraft(resource: Resource): Draft {
  return {
    name: resource.name,
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
    kind: "core:file",
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
  return `${option.label} (${option.kind})`;
}

export function kindOptionHint(option: ResourceKindOption | undefined): string {
  if (!option) return "";
  const content = option.supports_content ? "content" : "metadata only";
  const actions = option.actions.length ? ` / ${option.actions.map((action) => action.id).join(", ")}` : "";
  const detect = option.detect
    ? ` / detects ${[...option.detect.mime_types, ...option.detect.extensions].join(", ")}`
    : "";
  return `${option.source} / ${content}${option.schema_id ? ` / ${option.schema_id}` : ""}${detect}${actions}`;
}

export function isImageResource(resource: Resource): boolean {
  const mimeType = resource.content?.mime_type;
  return Boolean(mimeType && mimeType.startsWith("image/"));
}

export function isPluginUiAction(action: ResourceActionDefinition): boolean {
  return !["download_content", "read", "view_inline", "preview", "thumbnail"].includes(action.id);
}

export function detectKindForFile(file: File, options: ResourceKindOption[]): string {
  const mimeType = file.type.toLowerCase();
  const filename = file.name.toLowerCase();
  const matched = options
    .filter((option) => option.supports_content)
    .map((option) => ({ option, score: detectScore(option, mimeType, filename) }))
    .filter((match) => match.score > 0)
    .sort((left, right) => right.score - left.score)[0];

  return matched?.option.kind ?? fallbackUploadKind(options);
}

function detectScore(option: ResourceKindOption, mimeType: string, filename: string): number {
  let score = 0;
  score = Math.max(score, detectRuleScore(option.detect, mimeType, filename, 100));
  for (const action of option.actions) {
    score = Math.max(score, detectRuleScore(action.when, mimeType, filename, 0));
  }

  return score;
}

function detectRuleScore(
  detect: { mime_types: string[]; extensions: string[] } | undefined,
  mimeType: string,
  filename: string,
  base: number,
): number {
  if (!detect) return 0;

  let score = 0;
  for (const mimeTypeRule of detect.mime_types) {
    if (mimeTypeRule.trim().endsWith("/*") && mimeMatches(mimeTypeRule, mimeType)) {
      score = Math.max(score, base + 10);
    } else if (mimeMatches(mimeTypeRule, mimeType)) {
      score = Math.max(score, base + 20);
    }
  }
  for (const extension of detect.extensions) {
    if (filename.endsWith(normalizeExtension(extension))) {
      score = Math.max(score, base + 30);
    }
  }

  return score;
}

export function fallbackUploadKind(options: ResourceKindOption[]): string {
  return options.find((option) => option.kind === "core:file")?.kind ?? options[0]?.kind ?? "core:file";
}

function mimeMatches(rule: string, mimeType: string): boolean {
  const normalizedRule = rule.trim().toLowerCase();
  if (!normalizedRule || !mimeType) return false;
  if (normalizedRule === mimeType) return true;
  const prefix = normalizedRule.endsWith("/*") ? normalizedRule.slice(0, -1) : "";
  return Boolean(prefix && mimeType.startsWith(prefix));
}

function normalizeExtension(value: string): string {
  const extension = value.trim().toLowerCase();
  if (!extension) return extension;
  return extension.startsWith(".") ? extension : `.${extension}`;
}

export function buildResourceTreeRows(resources: Resource[]): ResourceTreeRow[] {
  const sorted = [...resources].sort((left, right) => resourceSortPath(left).localeCompare(resourceSortPath(right)));
  const folderCounts = new Map<string, number>();

  for (const resource of sorted) {
    const segments = resourceDirectorySegments(resource);
    for (let index = 0; index < segments.length; index += 1) {
      const path = segments.slice(0, index + 1).join("/");
      folderCounts.set(path, (folderCounts.get(path) ?? 0) + 1);
    }
  }

  const rows: ResourceTreeRow[] = [];
  const emittedFolders = new Set<string>();
  for (const resource of sorted) {
    const segments = resourceDirectorySegments(resource);
    for (let index = 0; index < segments.length; index += 1) {
      const path = segments.slice(0, index + 1).join("/");
      if (emittedFolders.has(path)) continue;
      emittedFolders.add(path);
      rows.push({
        type: "folder",
        path,
        name: segments[index],
        depth: index,
        count: folderCounts.get(path) ?? 0,
      });
    }

    rows.push({
      type: "resource",
      resource,
      depth: segments.length,
    });
  }

  return rows;
}

export function directoriesFromResources(resources: Resource[]): string[] {
  const directories = new Set<string>(["uploads"]);
  for (const resource of resources) {
    const segments = resourceDirectorySegments(resource);
    for (let index = 0; index < segments.length; index += 1) {
      directories.add(segments.slice(0, index + 1).join("/"));
    }
  }

  return [...directories].sort((left, right) => left.localeCompare(right));
}

function resourceDirectorySegments(resource: Resource): string[] {
  const key = resource.content?.key;
  if (!key || !key.includes("/")) return [];

  return key.split("/").slice(0, -1).filter(Boolean);
}

function resourceSortPath(resource: Resource): string {
  return resource.content?.key ?? `~metadata/${resource.name}`;
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

export async function sha256Hex(file: File): Promise<string> {
  const buffer = await file.arrayBuffer();
  const digest = await crypto.subtle.digest("SHA-256", buffer);
  return Array.from(new Uint8Array(digest))
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
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
