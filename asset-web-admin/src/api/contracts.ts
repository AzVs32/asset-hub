import type { components } from "./generated";
import type { PluginView, PluginViewKind } from "../plugins/host/contracts";

type Schemas = components["schemas"];

export type CurrentUser = Schemas["AuthenticatedUser"];
export type ManagedUser = Omit<Schemas["ManagedUserResponse"], "role" | "status"> & {
  role: "administrator" | "member";
  status: "active" | "disabled";
};
export type DirectoryAccessEntry = Omit<Schemas["DirectoryGrantResponse"], "permission"> & {
  permission: "read" | "write" | "full";
};

export type ResourceStatus = "active" | "archived";
export type ResourceActionDefinition = Omit<
  Schemas["ResourceActionDefinitionResponse"],
  "access" | "executor" | "output" | "requires"
> & {
  access: "read_only" | "read_write";
  executor: Omit<Schemas["ResourceActionExecutorResponse"], "type"> & {
    type: "builtin" | "plugin";
  };
  output: { view: PluginViewKind[] };
  requires: Omit<Schemas["ResourceActionRequirementsResponse"], "content_delivery"> & {
    content_delivery: "auto" | "inline" | "reference";
  };
};

export type Resource = Omit<Schemas["ResourceResponse"], "actions" | "status"> & {
  actions: { available_actions: ResourceActionDefinition[] };
  status: ResourceStatus;
};
export type ResourceKindOption = Omit<Schemas["ResourceKindResponse"], "actions"> & {
  actions: ResourceActionDefinition[];
};
export type ResourcePage = Omit<Schemas["ResourcePageResponse"], "items"> & {
  items: Resource[];
};
export type ResourceDirectory = Schemas["ResourceDirectoryResponse"];
export type DirectoryListing = Omit<Schemas["DirectoryListingResponse"], "resources"> & {
  resources: ResourcePage;
};
export type ResourceKindsResponse = Omit<Schemas["ResourceKindsResponse"], "items"> & {
  items: ResourceKindOption[];
};
export type ScanStorageResponse = Omit<Schemas["ScanStorageResponse"], "resources"> & {
  resources: Resource[];
};

export type PluginDiagnostic = Omit<Schemas["PluginDiagnosticResponse"], "severity"> & {
  severity: "info" | "warning" | "error";
};
export type PluginActionOutput = Omit<
  Schemas["ResourceActionOutputResponse"],
  "diagnostics" | "view"
> & {
  diagnostics: PluginDiagnostic[];
  view: PluginView;
};

