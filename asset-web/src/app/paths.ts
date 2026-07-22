import type { CurrentUser } from "@/domain/auth";

export const LOGIN_PATH = "/login";

export function directoryPath(directory = ""): string {
  if (!directory) return "/";
  const encoded = directory.split("/").filter(Boolean).map(encodeURIComponent).join("/");
  return encoded ? `/${encoded}` : "/";
}

export function defaultDirectoryPath(user: CurrentUser): string {
  return directoryPath(user.isAdmin ? "" : user.workspaceDirectory);
}
