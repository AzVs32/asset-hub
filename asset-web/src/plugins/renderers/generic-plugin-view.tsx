import { Box, CircularProgress } from "@mui/material";
import React from "react";
import type { AssetGateway } from "@/application/ports/asset-gateway";
import type { PluginView } from "@/domain/plugin";

const MarkdownRenderer = React.lazy(() => import("./markdown-renderer"));
const MediaRenderer = React.lazy(() => import("./media-renderer"));
const DownloadRenderer = React.lazy(() => import("./download-renderer"));

type GenericPluginView = Exclude<PluginView, { view: "plugin_frame" }>;

export function GenericPluginViewRenderer({
  view,
  gateway,
}: {
  view: GenericPluginView;
  gateway: AssetGateway;
}) {
  if (view.view === "text") return <TextView text={view.text} />;
  if (view.view === "markdown") {
    return (
      <LazyView>
        <MarkdownRenderer view={view} />
      </LazyView>
    );
  }
  if (view.view === "html") return <HtmlView view={view} />;
  if (view.view === "media") {
    return (
      <LazyView>
        <MediaRenderer view={view} gateway={gateway} />
      </LazyView>
    );
  }
  if (view.view === "download") {
    return (
      <LazyView>
        <DownloadRenderer view={view} gateway={gateway} />
      </LazyView>
    );
  }
  return <JsonView value={view.data} />;
}

function TextView({ text }: { text: string }) {
  return (
    <Box component="article" className="plugin-prose" sx={{ whiteSpace: "pre-wrap" }}>
      {text}
    </Box>
  );
}

function HtmlView({ view }: { view: Extract<PluginView, { view: "html" }> }) {
  return (
    <Box
      component="iframe"
      sandbox=""
      title={view.title ?? "Plugin HTML output"}
      srcDoc={htmlWithoutNetwork(view.html)}
      sx={{
        display: "block",
        height: "65vh",
        minHeight: "20rem",
        width: "100%",
        border: 0,
      }}
    />
  );
}

function JsonView({ value }: { value: unknown }) {
  return (
    <Box
      component="pre"
      sx={{
        maxHeight: "65vh",
        overflow: "auto",
        p: 2,
        fontFamily: "monospace",
        fontSize: "0.875rem",
        whiteSpace: "pre-wrap",
        m: 0,
      }}
    >
      {JSON.stringify(value, null, 2)}
    </Box>
  );
}

function htmlWithoutNetwork(html: string): string {
  const policy =
    "default-src 'none'; img-src data:; media-src data:; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'";
  return `<meta http-equiv="Content-Security-Policy" content="${policy}">${html}`;
}

function LazyView({ children }: { children: React.ReactNode }) {
  return (
    <React.Suspense
      fallback={
        <Box sx={{ display: "grid", minHeight: "10rem", placeItems: "center" }}>
          <CircularProgress size={20} />
        </Box>
      }
    >
      {children}
    </React.Suspense>
  );
}
