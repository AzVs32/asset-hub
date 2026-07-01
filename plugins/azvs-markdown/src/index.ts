import { buildMarkdownView, type PluginActionRequest } from "./markdown.js";

export function render_markdown(): void {
  const request = JSON.parse(Host.inputString()) as PluginActionRequest;
  const data = buildMarkdownView(request);

  Host.outputString(
    JSON.stringify({
      view: "markdown",
      markdown: data.source.markdown,
    }),
  );
}
