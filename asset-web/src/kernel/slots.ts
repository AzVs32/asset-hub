export const hostSlots = {
  resourceDetailActions: "resource_detail",
  resourceContextMenu: "context_menu",
  resourceListThumbnail: "resource_list_thumbnail",
  resourceDetailPanel: "resource_detail_panel",
  resourceDetailAside: "resource_detail_aside",
  directoryToolbar: "directory_toolbar",
  directoryContextMenu: "directory_context_menu",
  directoryDetail: "directory_detail",
} as const;

export type HostSlot = (typeof hostSlots)[keyof typeof hostSlots];

export const hostSlotCatalog: ReadonlyArray<{
  id: HostSlot;
  behavior: "action" | "menu" | "automatic_view";
  description: string;
}> = [
  {
    id: hostSlots.directoryToolbar,
    behavior: "action",
    description: "Actions for the currently open directory.",
  },
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
    id: hostSlots.resourceListThumbnail,
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
