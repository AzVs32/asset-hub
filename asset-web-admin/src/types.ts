export type ResourceStatus = "active" | "archived";

export type ResourceMetadata = {
  schema_version: number;
  summary: {
    description: string | null;
    tags: string[];
  };
};

export type ResourceContent = {
  key: string;
  size: number;
  mime_type: string | null;
  original_filename: string | null;
  checksum: Array<{ kind: string; value: string }>;
};

export type ResourceActions = {
  available_actions: ResourceActionDefinition[];
};

export type ResourceActionDefinition = {
  id: string;
  label: string;
  description: string | null;
  executor: {
    type: "builtin" | "plugin";
    handler: string | null;
  };
  access: "read_only" | "read_write";
  requires: {
    resource: boolean;
    metadata: boolean;
    content: boolean;
    content_delivery: "auto" | "inline" | "url";
  };
  output: {
    view: string[];
  };
  ui: {
    group: string | null;
    order: number | null;
    locations: string[];
  };
  applies_to: {
    kinds: string[];
    mime_types: string[];
    extensions: string[];
  };
};

export type Resource = {
  id: string;
  name: string;
  directory: string;
  kind: string;
  status: ResourceStatus;
  metadata: ResourceMetadata;
  content: ResourceContent | null;
  actions: ResourceActions;
  created_at: string;
  updated_at: string;
  deleted_at: string | null;
};

export type ResourceKindOption = {
  kind: string;
  parent: string | null;
  ancestors: string[];
  label: string;
  supports_content: boolean;
  detect?: {
    mime_types: string[];
    extensions: string[];
  };
  actions: ResourceActionDefinition[];
  source: string;
};

export type ResourceReadResponse = {
  id: string;
  name: string;
  kind: string;
  view: PluginView;
};

export type PluginActionOutput = {
  resource_id: string;
  action: string;
  view: PluginView;
};

export type PluginView =
  | {
      view: "text";
      text: string;
    }
  | {
      view: "markdown";
      markdown: string;
    }
  | {
      view: "html";
      title?: string;
      html: string;
    }
  | {
      view: "plugin_frame";
      title?: string;
      url: string;
    }
  | {
      view: "json";
      data: unknown;
    }
  | {
      view: "media";
      mime_type: string;
      title?: string;
      encoding: "base64" | "url";
      data: string;
    }
  | {
      view: "binary_url";
      url: string;
      mime_type?: string;
      filename?: string;
    }
  | {
      view: "table";
      columns: Array<{ key: string; label: string }>;
      rows: unknown[];
    }
  | {
      view: "form";
      schema: unknown;
      value?: unknown;
      submit_action?: string;
    };

export type ResourceKindsResponse = {
  items: ResourceKindOption[];
};

export type ResourcePage = {
  items: Resource[];
  total: number;
  page: number;
  limit: number;
};

export type ResourceDirectory = {
  path: string;
  parent_path: string;
  name: string;
};

export type DirectoryListing = {
  path: string;
  folders: ResourceDirectory[];
  resources: ResourcePage;
};

export type ScanStorageResponse = {
  path: string;
  scanned: number;
  imported: number;
  skipped: number;
  errors: Array<{ key: string; error: string }>;
  resources: Resource[];
};

export type Filters = {
  q: string;
  kind: string;
  tag: string;
  includeDeleted: boolean;
  includeDescendants: boolean;
  page: number;
  limit: number;
};

export type Draft = {
  name: string;
  directory: string;
  kind: string;
  status: ResourceStatus;
  description: string;
  tags: string;
};

export type UploadDraft = {
  file: File | null;
  name: string;
  kind: string;
  directory: string;
  description: string;
  tags: string;
};
