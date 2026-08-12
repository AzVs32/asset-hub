import type React from "react";

/**
 * The built-in Directory UI subtree. Its row menus, thumbnails, filters, and detail panel are
 * Core-owned implementation slots and are absent when a plugin owns directory_workspace.
 */
export function CoreDirectoryWorkspace({
  browser,
  detail,
}: {
  browser: React.ReactNode;
  detail: React.ReactNode;
}) {
  return (
    <div className="grid min-h-0 flex-1 gap-4 p-4 lg:grid-cols-[minmax(0,1fr)_minmax(23rem,27rem)] xl:gap-5 xl:p-5">
      {browser}
      {detail}
    </div>
  );
}
