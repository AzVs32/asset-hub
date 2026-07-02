export type ResourceStatus = "active" | "archived";

export type ResourceMetadata = {
  schema_version: number;
  summary: {
    description: string | null;
    tags: string[];
  };
  kind: {
    schema_id: string;
    data: Record<string, unknown>;
  } | null;
};

export type ResourceContent = {
  key: string;
  size: number;
  mime_type: string | null;
  original_filename: string | null;
  checksum: Array<{ kind: string; value: string }>;
};

export type ResourceActions = {
  download_content: boolean;
  read: boolean;
  view_inline: boolean;
  preview: boolean;
  thumbnail: boolean;
  available_actions: ResourceActionDefinition[];
};

export type ResourceActionDefinition = {
  id: string;
  label: string;
  access: "read_only" | "read_write";
  when?: {
    mime_types: string[];
    extensions: string[];
  };
};

export type Resource = {
  id: string;
  name: string;
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
  label: string;
  schema_id: string | null;
  metadata_schema: Record<string, unknown> | null;
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

export type Filters = {
  q: string;
  kind: string;
  tag: string;
  includeDeleted: boolean;
  page: number;
  limit: number;
};

export type Draft = {
  name: string;
  kind: string;
  status: ResourceStatus;
  description: string;
  tags: string;
  schemaId: string;
  kindData: string;
};

export type UploadDraft = {
  file: File | null;
  name: string;
  kind: string;
  directory: string;
  description: string;
  tags: string;
  schemaId: string;
  kindData: string;
};

export type ResourceTreeRow =
  | {
      type: "folder";
      path: string;
      name: string;
      depth: number;
      count: number;
    }
  | {
      type: "resource";
      resource: Resource;
      depth: number;
    };


