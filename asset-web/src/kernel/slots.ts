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

export const coreDirectoryWorkspaceSlotCatalog: ReadonlyArray<{
  id: CoreDirectoryWorkspaceSlot;
  behavior: "menu" | "automatic_view";
  description: string;
}> = [
  {
    id: coreDirectoryWorkspaceSlots.directoryContextMenu,
    behavior: "menu",
    description: "Entries in a CoreDirectoryWorkspace directory-row context menu.",
  },
  {
    id: coreDirectoryWorkspaceSlots.directoryThumbnail,
    behavior: "automatic_view",
    description: "Directory thumbnail rendered by CoreDirectoryWorkspace.",
  },
  {
    id: coreDirectoryWorkspaceSlots.resourceContextMenu,
    behavior: "menu",
    description: "Entries in a CoreDirectoryWorkspace resource-row context menu.",
  },
  {
    id: coreDirectoryWorkspaceSlots.resourceThumbnail,
    behavior: "automatic_view",
    description: "Resource thumbnail rendered by CoreDirectoryWorkspace.",
  },
];
