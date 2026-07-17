export const pluginViewKinds = [
  "text",
  "markdown",
  "html",
  "plugin_frame",
  "json",
  "media",
  "binary_url",
  "table",
  "form",
] as const;

export type PluginViewKind = (typeof pluginViewKinds)[number];
export type JsonObject = Record<string, unknown>;

export type PluginView =
  | { view: "text"; text: string }
  | { view: "markdown"; markdown: string }
  | { view: "html"; title?: string; html: string }
  | { view: "plugin_frame"; title?: string; url: string }
  | { view: "json"; data: unknown }
  | {
      view: "media";
      mime_type: string;
      title?: string;
      encoding: "base64" | "url";
      data: string;
    }
  | { view: "binary_url"; url: string; mime_type?: string; filename?: string }
  | { view: "table"; columns: Array<{ key: string; label: string }>; rows: unknown[] }
  | { view: "form"; schema: JsonObject; value?: unknown; submit_action?: string };

