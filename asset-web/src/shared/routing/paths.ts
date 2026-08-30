import { normalizeDirectory } from "@/domain/directory-path";

export const LOGIN_PATH = "/login";

export function directoryPath(directory = ""): string {
  if (!directory) return "/";
  const encoded = directory.split("/").filter(Boolean).map(encodeURIComponent).join("/");
  return encoded ? `/${encoded}` : "/";
}

export function defaultDirectoryPath(): string {
  return "/";
}

export function decodeDirectoryPath(pathname: string): string {
  return normalizeDirectory(
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
