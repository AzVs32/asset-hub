import type { DirectoryActionOutput, PluginView, ResourceActionOutput } from "../src/contract";
import type { AssetHubDirectoryFrameClient } from "../src/index";

const views: PluginView[] = [
  { view: "text", text: "plain" },
  { view: "markdown", markdown: "# Markdown" },
  { view: "html", title: "HTML", html: "<p>HTML</p>" },
  {
    view: "plugin_frame",
    plugin_api: "asset-hub.plugin-api@1",
    title: "Frame",
    url: "/plugins/example/index.html",
  },
  { view: "json", data: { nested: [true, 1, "value", null] } },
  { view: "media", mime_type: "image/png", encoding: "url", data: "/image.png" },
  { view: "download", url: "/download", filename: "example.bin" },
];

const resourceOutput: ResourceActionOutput = {
  resourceId: "resource-id",
  action: "example.resource.action",
  view: null,
  effects: ["replace_content", "delete"],
  diagnostics: [
    {
      code: "example.warning",
      message: "Example warning",
      severity: "warning",
      retryable: false,
      details: { source: "contract-test" },
    },
  ],
};

const directoryOutput: DirectoryActionOutput = {
  directoryId: "directory-id",
  action: "example.directory.action",
  view: views[0] ?? null,
  effects: ["update", "create_child", "create_tree", "delete"],
  diagnostics: [],
};

declare const directoryFrame: AssetHubDirectoryFrameClient;
const readResource: Promise<ResourceActionOutput> = directoryFrame.viewResource("resource-id", {
  operation: "load",
});
const editResource: Promise<void> = directoryFrame.editResource("resource-id");

void [views, resourceOutput, directoryOutput, readResource, editResource];
