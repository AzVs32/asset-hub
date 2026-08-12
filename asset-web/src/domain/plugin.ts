export const pluginViewKinds = [
  "text",
  "markdown",
  "html",
  "plugin_frame",
  "json",
  "media",
  "download",
] as const;

export type PluginViewKind = (typeof pluginViewKinds)[number];
export type JsonObject = Record<string, unknown>;

export type PluginView =
  | { view: "text"; text: string }
  | { view: "markdown"; markdown: string }
  | { view: "html"; title?: string; html: string }
  | { view: "plugin_frame"; plugin_api: string; title?: string; url: string }
  | { view: "json"; data: unknown }
  | {
      view: "media";
      mime_type: string;
      title?: string;
      encoding: "base64" | "url";
      data: string;
    }
  | { view: "download"; url: string; mime_type?: string; filename?: string };

export interface PluginDiagnostic {
  code: string;
  message: string;
  severity: "info" | "warning" | "error";
  retryable: boolean;
  details?: unknown;
}

export interface ResourceActionOutput {
  resourceId: string;
  action: string;
  view: PluginView | null;
  effects: import("./resource").ResourceActionEffectKind[];
  diagnostics: PluginDiagnostic[];
}

export interface DirectoryActionOutput {
  directoryId: string;
  action: string;
  view: PluginView | null;
  effects: import("./resource").DirectoryActionEffectKind[];
  diagnostics: PluginDiagnostic[];
}
