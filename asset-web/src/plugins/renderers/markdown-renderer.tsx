import ReactMarkdown from "react-markdown";
import type { PluginView } from "@/domain/plugin";

export default function MarkdownRenderer({
  view,
}: {
  view: Extract<PluginView, { view: "markdown" }>;
}) {
  return (
    <article className="plugin-prose markdown-body">
      <ReactMarkdown>{view.markdown}</ReactMarkdown>
    </article>
  );
}
