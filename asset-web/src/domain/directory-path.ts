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

export function parentDirectory(path: string): string | null {
  const normalized = normalizeDirectory(path);
  if (!normalized) return null;
  return normalized.split("/").slice(0, -1).join("/");
}
