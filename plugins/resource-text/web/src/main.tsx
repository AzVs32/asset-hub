import {
	type AssetHubFrameClient,
	connectAssetHubFrame,
	type JsonObject,
	PLUGIN_API_VERSION,
	type ResourceActionOutput,
} from "@asset-hub/plugin-web-sdk";
import { Check, Edit3, Eye, FileText, Menu, Save, X } from "lucide-react";
import MarkdownIt from "markdown-it";
import React from "react";
import { createRoot } from "react-dom/client";
import "./styles.css";

type FramePayload = {
	plugin_api: string;
	resource_id: string;
	mode: "read" | "edit";
	action: string;
	format: "markdown" | "plain";
};

type LoadResponseBase = {
	protocol: 1;
	resource_name: string;
	byte_length: number;
};

type LoadResponse = LoadResponseBase &
	(
		| { transfer: "complete"; text: string }
		| { transfer: "chunked"; chunk_size: number }
	);

type ChunkResponse = {
	protocol: 1;
	offset: number;
	byte_length: number;
	data: string;
	done: boolean;
};

type Heading = {
	id: string;
	level: number;
	title: string;
	line: number;
};

type Section = Heading & {
	markdown: string;
	all?: boolean;
};

type SaveState =
	| { status: "idle"; label: string }
	| { status: "saving"; label: string }
	| { status: "saved"; label: string }
	| { status: "error"; label: string };

type MarkdownToken = {
	type: string;
	tag: string;
	content: string;
	map: [number, number] | null;
	children?: Array<{ content: string }>;
};

const renderer = new MarkdownIt({
	html: false,
	linkify: true,
	typographer: true,
});

const initialPayload = readPayload();
const maxTextBytes = 128 * 1024 * 1024;
const maxChunkBytes = 4 * 1024 * 1024;

function App() {
	const payload = initialPayload;
	const [source, setSource] = React.useState<string | null>(null);
	const [resourceName, setResourceName] = React.useState("Text");
	const [loadError, setLoadError] = React.useState<string | null>(
		payload ? null : "Invalid text frame payload",
	);
	const [activeId, setActiveId] = React.useState("all");
	const [sidebarOpen, setSidebarOpen] = React.useState(
		payload?.mode === "read" && payload.format === "markdown",
	);
	const [saveState, setSaveState] = React.useState<SaveState>({
		status: "idle",
		label: "",
	});
	const contentRef = React.useRef<HTMLElement | null>(null);
	const mode = payload?.mode === "edit" ? "edit" : "read";
	const format = payload?.format ?? "plain";
	const isMarkdown = format === "markdown";
	const sections = React.useMemo(
		() => (isMarkdown ? splitSections(source ?? "") : []),
		[isMarkdown, source],
	);
	const activeSection =
		sections.find((section) => section.id === activeId) ?? sections[0];
	const renderedHtml = React.useMemo(
		() =>
			isMarkdown
				? renderer.render(
						mode === "edit"
							? (source ?? "")
							: (activeSection?.markdown ?? source ?? ""),
					)
				: "",
		[activeSection, isMarkdown, mode, source],
	);

	React.useEffect(() => {
		if (!payload) return;
		let active = true;
		loadText(payload)
			.then((loaded) => {
				if (!active) return;
				setResourceName(loaded.resourceName);
				setSource(loaded.text);
			})
			.catch((reason: unknown) => {
				if (active)
					setLoadError(
						reason instanceof Error ? reason.message : "Unable to load text",
					);
			});
		return () => {
			active = false;
		};
	}, []);

	React.useEffect(() => {
		document.title = resourceName;
	}, [resourceName]);

	React.useEffect(() => {
		if (isMarkdown && !sections.some((section) => section.id === activeId)) {
			setActiveId("all");
		}
	}, [activeId, isMarkdown, sections]);

	function selectSection(id: string) {
		setActiveId(id);
		setSidebarOpen(false);
		contentRef.current?.scrollTo({ top: 0, behavior: "smooth" });
	}

	async function saveText() {
		if (!payload || source === null) {
			setSaveState({ status: "error", label: "Missing save target" });
			return;
		}

		setSaveState({ status: "saving", label: "Saving" });
		try {
			await replaceResourceText(source);
			setSaveState({ status: "saved", label: "Saved" });
		} catch (error) {
			setSaveState({
				status: "error",
				label: error instanceof Error ? error.message : "Save failed",
			});
		}
	}

	if (loadError) {
		return (
			<StatusScreen
				tone="error"
				title="Unable to open text"
				detail={loadError}
			/>
		);
	}

	if (source === null) {
		return (
			<StatusScreen
				tone="loading"
				title="Opening text"
				detail="Loading document content"
			/>
		);
	}

	return (
		<div
			className={`app ${mode === "edit" ? "editing" : "reading"} ${format} ${sidebarOpen ? "sidebar-visible" : ""}`}
		>
			<header className="toolbar">
				{mode === "read" && isMarkdown ? (
					<button
						className="icon-button"
						type="button"
						title={sidebarOpen ? "Hide headings" : "Show headings"}
						aria-label={sidebarOpen ? "Hide headings" : "Show headings"}
						onClick={() => setSidebarOpen((open) => !open)}
					>
						{sidebarOpen ? <X size={18} /> : <Menu size={18} />}
					</button>
				) : mode === "edit" ? (
					<div className="toolbar-mode" title="Edit mode" aria-hidden="true">
						<Edit3 size={18} />
					</div>
				) : (
					<div className="toolbar-mode" title="Plain text" aria-hidden="true">
						<FileText size={18} />
					</div>
				)}
				<div className="toolbar-title">
					<FileText size={18} />
					<span>{resourceName}</span>
				</div>
				{mode === "edit" ? (
					<div className="save-controls">
						{saveState.label && (
							<span className={`save-status ${saveState.status}`}>
								{saveState.status === "saved" && <Check size={14} />}
								{saveState.label}
							</span>
						)}
						<button
							className="primary-button"
							type="button"
							disabled={saveState.status === "saving"}
							onClick={() => void saveText()}
						>
							<Save size={16} />
							Save
						</button>
					</div>
				) : (
					<div className="toolbar-mode" title="Read mode" aria-hidden="true">
						<Eye size={18} />
					</div>
				)}
			</header>

			{mode === "read" && isMarkdown && (
				<aside className="sidebar" aria-label="Document headings">
					<nav className="heading-list">
						{sections.map((section) => (
							<button
								className={`heading-link ${section.all ? "all" : `depth-${Math.min(section.level, 6)}`} ${
									activeSection?.id === section.id ? "active" : ""
								}`}
								type="button"
								key={section.id}
								onClick={() => selectSection(section.id)}
							>
								{section.title}
							</button>
						))}
					</nav>
				</aside>
			)}

			{mode === "edit" && (
				<section
					className="editor-pane"
					aria-label={isMarkdown ? "Markdown source" : "Text source"}
				>
					<textarea
						spellCheck={false}
						value={source}
						onChange={(event) => {
							setSource(event.target.value);
							if (saveState.status !== "idle") {
								setSaveState({ status: "idle", label: "" });
							}
						}}
					/>
				</section>
			)}

			{(mode === "read" || isMarkdown) && (
				<main className="content-scroll" ref={contentRef}>
					{isMarkdown ? (
						<article
							className="markdown-body"
							// biome-ignore lint/security/noDangerouslySetInnerHtml: MarkdownIt escapes raw HTML because its html option is disabled.
							dangerouslySetInnerHTML={{ __html: renderedHtml }}
						/>
					) : (
						<pre className="plain-text-body">{source}</pre>
					)}
				</main>
			)}
		</div>
	);
}

function StatusScreen({
	tone,
	title,
	detail,
}: {
	tone: "loading" | "error";
	title: string;
	detail: string;
}) {
	return (
		<main className={`status-screen ${tone}`}>
			<FileText size={32} />
			<h1>{title}</h1>
			<p>{detail}</p>
		</main>
	);
}

function readPayload(): FramePayload | null {
	try {
		const params = new URLSearchParams(window.location.hash.replace(/^#/, ""));
		const encoded = params.get("payload");
		if (!encoded) return null;
		const base64 = encoded.replace(/-/g, "+").replace(/_/g, "/");
		const padded = base64 + "=".repeat((4 - (base64.length % 4)) % 4);
		const bytes = Uint8Array.from(atob(padded), (character) =>
			character.charCodeAt(0),
		);
		const value = JSON.parse(
			new TextDecoder().decode(bytes),
		) as Partial<FramePayload>;
		if (
			value.plugin_api !== PLUGIN_API_VERSION ||
			typeof value.resource_id !== "string" ||
			(value.mode !== "read" && value.mode !== "edit") ||
			typeof value.action !== "string" ||
			(value.format !== "markdown" && value.format !== "plain")
		)
			return null;
		return value as FramePayload;
	} catch {
		return null;
	}
}

async function loadText(
	payload: FramePayload,
): Promise<{ resourceName: string; text: string }> {
	const load = await executeResourceAction(payload, { operation: "load" });
	const description = jsonViewData(
		load,
		isLoadResponse,
		"Text plugin returned an invalid load response",
	);
	if (description.transfer === "complete") {
		return {
			resourceName: description.resource_name,
			text: description.text.replace(/^\uFEFF/, ""),
		};
	}

	const bytes = new Uint8Array(description.byte_length);
	let offset = 0;
	while (offset < description.byte_length) {
		const result = await executeResourceAction(payload, {
			operation: "chunk",
			offset,
		});
		const chunk = jsonViewData(
			result,
			isChunkResponse,
			"Text plugin returned an invalid chunk",
		);
		if (
			chunk.offset !== offset ||
			chunk.byte_length !== description.byte_length
		) {
			throw new Error("Text chunk sequence does not match the document");
		}
		const chunkBytes = decodeBase64(chunk.data);
		const expectedLength = Math.min(
			description.chunk_size,
			description.byte_length - offset,
		);
		if (
			chunkBytes.length !== expectedLength ||
			chunk.done !== (offset + expectedLength === description.byte_length)
		) {
			throw new Error("Text chunk has an invalid length");
		}
		bytes.set(chunkBytes, offset);
		offset += chunkBytes.length;
	}

	let text: string;
	try {
		text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
	} catch {
		throw new Error("Text content is not valid UTF-8");
	}
	return {
		resourceName: description.resource_name,
		text: text.replace(/^\uFEFF/, ""),
	};
}

function jsonViewData<T>(
	result: ResourceActionOutput,
	validate: (value: unknown) => value is T,
	invalidMessage: string,
): T {
	const view = result.view;
	if (view?.view !== "json" || !validate(view.data)) {
		throw new Error(invalidMessage);
	}
	return view.data;
}

function isLoadResponse(value: unknown): value is LoadResponse {
	if (!value || typeof value !== "object") return false;
	const response = value as Partial<LoadResponseBase> & {
		transfer?: unknown;
		text?: unknown;
		chunk_size?: unknown;
	};
	if (
		response.protocol !== 1 ||
		typeof response.resource_name !== "string" ||
		!isByteLength(response.byte_length)
	)
		return false;
	if (response.transfer === "complete")
		return typeof response.text === "string";
	return (
		response.transfer === "chunked" &&
		typeof response.chunk_size === "number" &&
		Number.isSafeInteger(response.chunk_size) &&
		response.chunk_size > 0 &&
		response.chunk_size <= maxChunkBytes
	);
}

function isChunkResponse(value: unknown): value is ChunkResponse {
	if (!value || typeof value !== "object") return false;
	const response = value as Partial<ChunkResponse>;
	return (
		response.protocol === 1 &&
		typeof response.offset === "number" &&
		Number.isSafeInteger(response.offset) &&
		response.offset >= 0 &&
		isByteLength(response.byte_length) &&
		typeof response.data === "string" &&
		typeof response.done === "boolean"
	);
}

function isByteLength(value: unknown): value is number {
	return (
		Number.isSafeInteger(value) &&
		(value as number) >= 0 &&
		(value as number) <= maxTextBytes
	);
}

function decodeBase64(value: string): Uint8Array {
	try {
		return Uint8Array.from(atob(value), (character) => character.charCodeAt(0));
	} catch {
		throw new Error("Text chunk is not valid Base64");
	}
}

function splitSections(markdownSource: string): Section[] {
	const lines = markdownSource.replace(/\r\n?/g, "\n").split("\n");
	const headings = collectHeadings(markdownSource);
	const allSection: Section = {
		id: "all",
		level: 0,
		title: "All content",
		line: 0,
		markdown: markdownSource,
		all: true,
	};

	if (headings.length === 0) {
		return [allSection];
	}

	return [
		allSection,
		...headings.map((heading, index) => {
			const next = headings
				.slice(index + 1)
				.find((item) => item.level <= heading.level);
			const start = heading.line;
			const end = next ? next.line : lines.length;
			return {
				...heading,
				markdown: lines.slice(start, end).join("\n"),
			};
		}),
	];
}

function collectHeadings(markdownSource: string): Heading[] {
	const tokens = renderer.parse(markdownSource, {}) as MarkdownToken[];
	const headings: Heading[] = [];
	const usedIds = new Map<string, number>([["all", 1]]);

	for (let index = 0; index < tokens.length; index += 1) {
		const token = tokens[index];
		if (token.type !== "heading_open") continue;
		const inline = tokens[index + 1];
		const level = headingLevel(token.tag);
		const title = plainText(inline);
		if (!title) continue;
		const baseId = slugify(title) || `heading-${headings.length + 1}`;
		const count = usedIds.get(baseId) ?? 0;
		usedIds.set(baseId, count + 1);
		headings.push({
			id: count === 0 ? baseId : `${baseId}-${count + 1}`,
			level,
			title,
			line: token.map?.[0] ?? 0,
		});
	}

	return headings;
}

function headingLevel(tag: string): number {
	const level = Number(tag.replace(/^h/, ""));
	return Number.isFinite(level) ? level : 1;
}

function plainText(token: MarkdownToken | undefined): string {
	if (!token) return "";
	if (token.children?.length) {
		return token.children
			.map((child) => child.content)
			.join("")
			.trim();
	}
	return token.content.trim();
}

function slugify(value: string): string {
	return value
		.toLowerCase()
		.trim()
		.replace(/[^\p{Letter}\p{Number}\s-]/gu, "")
		.replace(/\s+/g, "-")
		.replace(/-+/g, "-");
}

function executeResourceAction(
	frame: FramePayload,
	input: JsonObject,
): Promise<ResourceActionOutput> {
	return frameHost().then((host) =>
		host.executeResourceAction(frame.action, input),
	);
}

function replaceResourceText(text: string): Promise<void> {
	return frameHost().then((host) => host.replaceResourceText(text));
}

let frameHostPromise: Promise<AssetHubFrameClient> | null = null;

function frameHost(): Promise<AssetHubFrameClient> {
	frameHostPromise ??= connectAssetHubFrame();
	return frameHostPromise;
}

const root = document.getElementById("root");
if (!root) throw new Error("Missing Resource Text root element");
createRoot(root).render(<App />);
