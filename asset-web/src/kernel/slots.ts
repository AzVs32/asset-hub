/** The single Host-owned handoff point for an entire Directory workspace. */
export const directoryWorkspaceOutlet = "directory_workspace" as const;

/** Slots owned by the built-in CoreDirectoryWorkspace implementation only. */
export const coreDirectoryWorkspaceSlots = {
  resourceContextMenu: "resource_context_menu",
  resourceThumbnail: "resource_thumbnail",
  directoryContextMenu: "directory_context_menu",
  directoryThumbnail: "directory_thumbnail",
} as const;

export type CoreDirectoryWorkspaceSlot =
  (typeof coreDirectoryWorkspaceSlots)[keyof typeof coreDirectoryWorkspaceSlots];
