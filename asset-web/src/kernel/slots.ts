export const hostSlots = {
  resourceDetailActions: "resource_detail",
  resourceContextMenu: "context_menu",
  resourceThumbnail: "resource_thumbnail",
  resourceDetailPanel: "resource_detail_panel",
  resourceDetailAside: "resource_detail_aside",
  directoryContextMenu: "directory_context_menu",
  directoryDetail: "directory_detail",
  directoryThumbnail: "directory_thumbnail",
} as const;

export type HostSlot = (typeof hostSlots)[keyof typeof hostSlots];

export const hostSlotCatalog: ReadonlyArray<{
  id: HostSlot;
  behavior: "action" | "menu" | "automatic_view";
  description: string;
}> = [
  {
    id: hostSlots.directoryContextMenu,
    behavior: "menu",
    description: "Entries in a directory row context menu.",
  },
  {
    id: hostSlots.directoryDetail,
    behavior: "action",
    description: "Actions in the selected directory detail panel.",
  },
  {
    id: hostSlots.directoryThumbnail,
    behavior: "automatic_view",
    description: "Read-only view used as the directory list thumbnail.",
  },
  {
    id: hostSlots.resourceDetailActions,
    behavior: "action",
    description: "Buttons in the resource detail action bar.",
  },
  {
    id: hostSlots.resourceContextMenu,
    behavior: "menu",
    description: "Entries in a resource row context menu.",
  },
  {
    id: hostSlots.resourceThumbnail,
    behavior: "automatic_view",
    description: "Read-only view used as the resource list thumbnail.",
  },
  {
    id: hostSlots.resourceDetailPanel,
    behavior: "automatic_view",
    description: "Read-only plugin content rendered below resource facts.",
  },
  {
    id: hostSlots.resourceDetailAside,
    behavior: "automatic_view",
    description: "Read-only plugin content rendered in the resource aside.",
  },
];
