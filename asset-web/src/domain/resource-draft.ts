import type { Resource, ResourceDraft, ResourceKind } from "./resource";

export function draftFromResource(resource: Resource): ResourceDraft {
  return {
    name: resource.name,
    directory: resource.directory,
    kind: resource.kind,
    status: resource.status,
    description: resource.description ?? "",
    tags: resource.tags.join(", "),
  };
}

export function emptyResourceDraft(directory: string, kinds: ResourceKind[]): ResourceDraft {
  return {
    name: "",
    directory,
    kind: kinds[0]?.kind ?? "core:unknown",
    status: "active",
    description: "",
    tags: "",
  };
}

export function normalizeDirectory(value: string): string {
  return value
    .trim()
    .replace(/\\/g, "/")
    .split("/")
    .map((part) => part.trim())
    .filter((part) => part && part !== ".")
    .join("/");
}

export function breadcrumbs(path: string, root = ""): Array<{ path: string; label: string }> {
  const normalizedRoot = normalizeDirectory(root);
  const normalizedPath = normalizeDirectory(path);
  const rootSegments = normalizedRoot ? normalizedRoot.split("/") : [];
  const pathSegments = normalizedPath ? normalizedPath.split("/") : [];
  const relative = pathSegments.slice(rootSegments.length);
  const result = [
    { path: normalizedRoot, label: normalizedRoot ? (rootSegments.at(-1) ?? "Root") : "Root" },
  ];
  for (let index = 0; index < relative.length; index += 1) {
    const parts = [...rootSegments, ...relative.slice(0, index + 1)];
    result.push({ path: parts.join("/"), label: relative[index] ?? "" });
  }
  return result;
}

export function parentDirectory(path: string, root = ""): string | null {
  const normalizedPath = normalizeDirectory(path);
  const normalizedRoot = normalizeDirectory(root);
  if (normalizedPath === normalizedRoot) return null;
  const parent = normalizedPath.split("/").slice(0, -1).join("/");
  return parent.length < normalizedRoot.length ? normalizedRoot : parent;
}

export function formatBytes(value: number): string {
  if (!value) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const exponent = Math.min(Math.floor(Math.log(value) / Math.log(1024)), units.length - 1);
  const amount = value / 1024 ** exponent;
  return `${amount >= 10 || exponent === 0 ? amount.toFixed(0) : amount.toFixed(1)} ${units[exponent]}`;
}

export function formatDate(value: string): string {
  const date = new Date(value);
  return Number.isNaN(date.valueOf())
    ? value
    : new Intl.DateTimeFormat(undefined, {
        dateStyle: "medium",
        timeStyle: "short",
      }).format(date);
}
