export type DirectoryBreadcrumb = { label: string; path: string };

export function directoryBreadcrumbs(
  directory: string,
  rootDirectory: string,
  rootLabel: string,
): DirectoryBreadcrumb[];

export function parentDirectoryWithinRoot(
  directory: string,
  rootDirectory: string,
): string | null;
