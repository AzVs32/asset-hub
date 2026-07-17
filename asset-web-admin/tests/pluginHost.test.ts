import { describe, expect, it } from "vitest";
import type { Resource, ResourceActionDefinition } from "../src/api/contracts";
import {
  actionLocations,
  actionsAt,
  executeResourceAction,
  selectThumbnailAction,
} from "../src/plugins/host/actions";
import { parseExecuteActionMessage } from "../src/plugins/host/frameProtocol";

describe("plugin action placement", () => {
  it("places unknown host locations in the detail fallback", () => {
    const resource = fixtureResource([
      fixtureAction("example.inspect", ["future_sidebar"], ["json"]),
    ]);
    expect(actionsAt(resource, actionLocations.resourceDetail, true).map((action) => action.id))
      .toEqual(["example.inspect"]);
  });

  it("selects thumbnail actions by declared location or media capability", () => {
    const explicit = fixtureAction("example.cover", [actionLocations.resourceListThumbnail], ["media"]);
    expect(selectThumbnailAction(fixtureResource([explicit]))?.id).toBe("example.cover");

    const media = fixtureAction("example.image", [actionLocations.resourceDetail], ["media"]);
    expect(selectThumbnailAction(fixtureResource([media], "image/png"))?.id).toBe("example.image");
  });

  it("adapts synthesized content actions without relying on their id", async () => {
    const fallback = {
      ...fixtureAction("server.generated.action", [], []),
      executor: { type: "builtin" as const, handler: null },
    };
    const resource = fixtureResource([fallback]);
    await expect(executeResourceAction(resource, fallback.id)).resolves.toMatchObject({
      action: fallback.id,
      view: { view: "binary_url", url: "/resources/resource-1/content" },
    });
  });
});

describe("plugin frame protocol", () => {
  it("accepts versioned cross-action requests and legacy messages", () => {
    const request = {
      type: "asset-hub:execute-resource-action",
      version: 1,
      request_id: "request-1",
      resource_id: "resource-1",
      action: "example.save",
      input: { text: "updated" },
    };
    expect(parseExecuteActionMessage(request)).toEqual(request);
    expect(parseExecuteActionMessage({ ...request, version: undefined })).not.toBeNull();
  });

  it("rejects unsupported protocol versions and non-object input", () => {
    const request = {
      type: "asset-hub:execute-resource-action",
      version: 2,
      request_id: "request-1",
      resource_id: "resource-1",
      action: "example.save",
    };
    expect(parseExecuteActionMessage(request)).toBeNull();
    expect(parseExecuteActionMessage({ ...request, version: 1, input: [] })).toBeNull();
  });
});

function fixtureAction(
  id: string,
  locations: string[],
  views: ResourceActionDefinition["output"]["view"],
): ResourceActionDefinition {
  return {
    id,
    label: id,
    description: null,
    executor: { type: "plugin", handler: id },
    access: "read_only",
    requires: { content: false, content_delivery: "auto" },
    output: { view: views },
    ui: { group: null, order: null, locations },
    applies_to: { kinds: [], mime_types: [], extensions: [] },
  };
}

function fixtureResource(actions: ResourceActionDefinition[], mimeType = "application/octet-stream"): Resource {
  return {
    id: "resource-1",
    name: "Resource",
    directory: "",
    kind: "example:file",
    status: "active",
    metadata: { schema_version: 1, summary: { description: null, tags: [] } },
    content: { key: "resource.bin", size: 1, mime_type: mimeType, original_filename: null, checksum: [] },
    actions: { available_actions: actions },
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    deleted_at: null,
  };
}
