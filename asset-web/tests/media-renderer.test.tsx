import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { AssetGateway } from "@/application/ports/asset-gateway";
import type { PluginView, ResourceActionOutput } from "@/domain/plugin";
import DownloadRenderer from "@/plugins/renderers/download-renderer";
import MediaRenderer from "@/plugins/renderers/media-renderer";
import { resource } from "./fixtures";

const gateway = {
  assetUrl: (path: string) => (path.startsWith("/") ? `/api${path}` : null),
} as AssetGateway;

describe("MediaRenderer", () => {
  it("renders download views as downloads without media preview", () => {
    const view: PluginView = {
      view: "download",
      url: "/resources/resource-1/download",
      mime_type: "video/mp4",
      filename: "demo.mp4",
    };

    const { container } = renderView(view, DownloadRenderer);

    expect(screen.getByRole("link", { name: "Download file" })).toHaveAttribute(
      "download",
      "demo.mp4",
    );
    expect(container.querySelector("video")).toBeNull();
  });

  it("does not provide a host video player for media views", () => {
    const view: PluginView = {
      view: "media",
      mime_type: "video/mp4",
      encoding: "url",
      data: "/resources/resource-1/content",
    };

    const { container } = renderView(view, MediaRenderer);

    expect(container.querySelector("video")).toBeNull();
    expect(screen.getByText(/No host renderer is available/)).toBeInTheDocument();
  });
});

function renderView(view: PluginView, Renderer: typeof MediaRenderer | typeof DownloadRenderer) {
  const output: ResourceActionOutput = {
    resourceId: "resource-1",
    action: "test.action",
    view,
    diagnostics: [],
  };
  return render(<Renderer view={view} output={output} resource={resource()} gateway={gateway} />);
}
