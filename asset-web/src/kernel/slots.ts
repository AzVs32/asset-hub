export const hostSlots = {
  resourceContextMenu: "resource_context_menu",
  resourceThumbnail: "resource_thumbnail",
  directoryContextMenu: "directory_context_menu",
  directoryThumbnail: "directory_thumbnail",
} as const;

export type HostSlot = (typeof hostSlots)[keyof typeof hostSlots];

export const hostSlotCatalog: ReadonlyArray<{
  id: HostSlot;
  behavior: "menu" | "automatic_view";
  description: string;
}> = [
  {
    id: hostSlots.directoryContextMenu,
    behavior: "menu",
    description: "Entries in a directory row context menu.",
  },
  {
    id: hostSlots.directoryThumbnail,
    behavior: "automatic_view",
    description: "Read-only view used as the directory thumbnail.",
  },
  {
    id: hostSlots.resourceContextMenu,
    behavior: "menu",
    description: "Entries in a resource row context menu.",
  },
  {
    id: hostSlots.resourceThumbnail,
    behavior: "automatic_view",
    description: "Read-only view used as the resource thumbnail.",
  },
];
