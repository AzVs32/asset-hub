import ReactMarkdown from "react-markdown";
import type { PluginViewRendererProps } from "@/kernel/plugin-kernel";

export default function MarkdownRenderer({ view }: PluginViewRendererProps) {
  if (view.view !== "markdown") return null;
  return (
    <article className="plugin-prose markdown-body">
      <ReactMarkdown>{view.markdown}</ReactMarkdown>
    </article>
  );
}
