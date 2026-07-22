declare const visibleDirectoryBrand: unique symbol;
declare const storageDirectoryBrand: unique symbol;

export type VisibleDirectory = string & { readonly [visibleDirectoryBrand]: true };
export type StorageDirectory = string & { readonly [storageDirectoryBrand]: true };

export function normalizeDirectory(value: string): string {
  const segments: string[] = [];
  for (const part of value.replace(/\\/g, "/").split("/")) {
    if (!part || part === ".") continue;
    if (part === "..") {
      segments.pop();
      continue;
    }
    segments.push(part);
  }
  return segments.join("/");
}

export function visibleDirectory(value = ""): VisibleDirectory {
  return normalizeDirectory(value) as VisibleDirectory;
}

export function storageDirectory(value = ""): StorageDirectory {
  return normalizeDirectory(value) as StorageDirectory;
}

export function breadcrumbs(path: string): Array<{ path: VisibleDirectory; label: string }> {
  const segments = normalizeDirectory(path).split("/").filter(Boolean);
  const result: Array<{ path: VisibleDirectory; label: string }> = [
    { path: visibleDirectory(), label: "Root" },
  ];
  for (let index = 0; index < segments.length; index += 1) {
    result.push({
      path: visibleDirectory(segments.slice(0, index + 1).join("/")),
      label: segments[index] ?? "",
    });
  }
  return result;
}

export function parentDirectory(path: string): VisibleDirectory | null {
  const normalized = normalizeDirectory(path);
  if (!normalized) return null;
  return visibleDirectory(normalized.split("/").slice(0, -1).join("/"));
}
