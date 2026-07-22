import type { WorkspaceScope } from "@/application/workspace/workspace-scope";
import { type VisibleDirectory, visibleDirectory } from "@/domain/directory-path";

export const LOGIN_PATH = "/login";

export function directoryPath(directory: VisibleDirectory): string {
  if (!directory) return "/";
  const encoded = directory.split("/").filter(Boolean).map(encodeURIComponent).join("/");
  return encoded ? `/${encoded}` : "/";
}

export function defaultDirectoryPath(): string {
  return "/";
}

export function canonicalWorkspaceLocation(location: string, scope: WorkspaceScope): string {
  const suffixIndex = location.search(/[?#]/);
  const pathname = suffixIndex < 0 ? location : location.slice(0, suffixIndex);
  const suffix = suffixIndex < 0 ? "" : location.slice(suffixIndex);
  const routeDirectory = decodeDirectoryPath(pathname);
  const canonical = canonicalDirectoryRoute(routeDirectory, scope);
  return canonical.changed ? `${directoryPath(canonical.directory)}${suffix}` : location;
}

export function canonicalDirectoryRoute(
  directory: VisibleDirectory,
  scope: WorkspaceScope,
): { directory: VisibleDirectory; changed: boolean } {
  if (scope.isGlobal) return { directory, changed: false };
  const visible = scope.tryToVisibleDirectory(directory);
  return visible === null ? { directory, changed: false } : { directory: visible, changed: true };
}

export function decodeDirectoryPath(pathname: string): VisibleDirectory {
  return visibleDirectory(
    pathname
      .split("/")
      .filter(Boolean)
      .map((segment) => {
        try {
          return decodeURIComponent(segment);
        } catch {
          return segment;
        }
      })
      .join("/"),
  );
}
