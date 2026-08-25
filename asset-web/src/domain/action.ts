export type ActionAccess = "read" | "write";

export interface ActionUi {
  group: string | null;
  order: number | null;
  locations: string[];
  destructive: boolean;
  confirmation: string | null;
}

export interface DefinitionOrigin {
  kind: "builtin" | "plugin";
  id: string;
}
