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

export function breadcrumbs(path: string): Array<{ path: string; label: string }> {
  const segments = normalizeDirectory(path).split("/").filter(Boolean);
  const result = [{ path: "", label: "Root" }];
  for (let index = 0; index < segments.length; index += 1) {
    result.push({
      path: segments.slice(0, index + 1).join("/"),
      label: segments[index] ?? "",
    });
  }
  return result;
}

export function parentDirectory(path: string): string | null {
  const normalized = normalizeDirectory(path);
  if (!normalized) return null;
  return normalized.split("/").slice(0, -1).join("/");
}
