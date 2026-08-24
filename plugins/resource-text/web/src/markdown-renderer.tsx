import MarkdownIt from "markdown-it";
import React from "react";
import { renderEmbeddedMermaidDiagrams } from "./mermaid-renderer";

type MarkdownHeading = {
	id: string;
	level: number;
	title: string;
	line: number;
};

type MarkdownToken = {
	type: string;
	tag: string;
	content: string;
	map: [number, number] | null;
	children?: Array<{ content: string }>;
};

export type MarkdownSection = MarkdownHeading & {
	markdown: string;
	all?: boolean;
};

export const ALL_MARKDOWN_SECTION_ID = "all";

const markdownRenderer = new MarkdownIt({
	html: false,
	linkify: true,
	typographer: true,
});

const defaultFenceRenderer = markdownRenderer.renderer.rules.fence;
markdownRenderer.renderer.rules.fence = (
	tokens,
	index,
	options,
	environment,
	self,
) => {
	const token = tokens[index];
	const language = token.info.trim().split(/\s+/, 1)[0]?.toLowerCase();
	if (language !== "mermaid") {
		return defaultFenceRenderer
			? defaultFenceRenderer(tokens, index, options, environment, self)
			: self.renderToken(tokens, index, options);
	}
	return `<figure class="mermaid-diagram" data-mermaid-diagram><pre class="mermaid-source" data-mermaid-source>${markdownRenderer.utils.escapeHtml(token.content)}</pre></figure>`;
};

// Mermaid replaces its source placeholders with SVG. Keep that rendered DOM intact
// when parent-only layout state, such as the headings sidebar, changes.
export const MarkdownDocument = React.memo(function MarkdownDocument({
	source,
}: {
	source: string;
}) {
	const articleRef = React.useRef<HTMLElement>(null);
	const html = React.useMemo(() => markdownRenderer.render(source), [source]);

	React.useEffect(() => {
		if (!html.includes("data-mermaid-diagram")) return;
		const article = articleRef.current;
		if (!article) return;
		return renderEmbeddedMermaidDiagrams(article);
	}, [html]);

	return (
		<article
			ref={articleRef}
			className="markdown-body"
			// biome-ignore lint/security/noDangerouslySetInnerHtml: MarkdownIt escapes raw HTML and Mermaid runs in strict mode inside the sandboxed plugin frame.
			dangerouslySetInnerHTML={{ __html: html }}
		/>
	);
});

export function buildMarkdownSections(
	markdownSource: string,
): MarkdownSection[] {
	const lines = markdownSource.replace(/\r\n?/g, "\n").split("\n");
	const headings = collectMarkdownHeadings(markdownSource);
	const allContent: MarkdownSection = {
		id: ALL_MARKDOWN_SECTION_ID,
		level: 0,
		title: "All content",
		line: 0,
		markdown: markdownSource,
		all: true,
	};

	if (headings.length === 0) return [allContent];

	return [
		allContent,
		...headings.map((heading, index) => {
			const next = headings
				.slice(index + 1)
				.find((candidate) => candidate.level <= heading.level);
			const end = next ? next.line : lines.length;
			return {
				...heading,
				markdown: lines.slice(heading.line, end).join("\n"),
			};
		}),
	];
}

function collectMarkdownHeadings(markdownSource: string): MarkdownHeading[] {
	const tokens = markdownRenderer.parse(markdownSource, {}) as MarkdownToken[];
	const headings: MarkdownHeading[] = [];
	const usedIds = new Map<string, number>([[ALL_MARKDOWN_SECTION_ID, 1]]);

	for (let index = 0; index < tokens.length; index += 1) {
		const token = tokens[index];
		if (token.type !== "heading_open") continue;
		const title = markdownTokenText(tokens[index + 1]);
		if (!title) continue;
		const baseId = markdownHeadingId(title) || `heading-${headings.length + 1}`;
		const count = usedIds.get(baseId) ?? 0;
		usedIds.set(baseId, count + 1);
		headings.push({
			id: count === 0 ? baseId : `${baseId}-${count + 1}`,
			level: markdownHeadingLevel(token.tag),
			title,
			line: token.map?.[0] ?? 0,
		});
	}

	return headings;
}

function markdownHeadingLevel(tag: string): number {
	const level = Number(tag.replace(/^h/, ""));
	return Number.isFinite(level) ? level : 1;
}

function markdownTokenText(token: MarkdownToken | undefined): string {
	if (!token) return "";
	if (token.children?.length) {
		return token.children
			.map((child) => child.content)
			.join("")
			.trim();
	}
	return token.content.trim();
}

function markdownHeadingId(value: string): string {
	return value
		.toLowerCase()
		.trim()
		.replace(/[^\p{Letter}\p{Number}\s-]/gu, "")
		.replace(/\s+/g, "-")
		.replace(/-+/g, "-");
}
