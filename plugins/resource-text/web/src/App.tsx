import {
	Check,
	Edit3,
	Eye,
	FileText,
	PanelLeftClose,
	PanelLeftOpen,
	Save,
} from "lucide-react";
import React from "react";
import {
	ALL_MARKDOWN_SECTION_ID,
	buildMarkdownSections,
	MarkdownDocument,
} from "./markdown-renderer";
import { MermaidPreview } from "./mermaid-renderer";
import {
	loadResourceText,
	saveResourceText,
	type TextDocumentFormat,
	type TextFrameContext,
} from "./text-frame-client";

type SaveState =
	| { status: "idle" }
	| { status: "saving" }
	| { status: "saved" }
	| { status: "error"; message: string };

export function ResourceTextApp({ frame }: { frame: TextFrameContext | null }) {
	const [documentText, setDocumentText] = React.useState<string | null>(null);
	const [resourceName, setResourceName] = React.useState("Text");
	const [loadError, setLoadError] = React.useState<string | null>(
		frame ? null : "Invalid text frame payload",
	);
	const [activeSectionId, setActiveSectionId] = React.useState(
		ALL_MARKDOWN_SECTION_ID,
	);
	const [sidebarOpen, setSidebarOpen] = React.useState(
		frame?.mode === "read" && frame.format === "markdown",
	);
	const [saveState, setSaveState] = React.useState<SaveState>({
		status: "idle",
	});
	const contentScrollerRef = React.useRef<HTMLElement | null>(null);
	const mode = frame?.mode ?? "read";
	const documentFormat = frame?.format ?? "plain";
	const isMarkdown = documentFormat === "markdown";
	const isMermaid = documentFormat === "mermaid";
	const markdownSections = React.useMemo(
		() => (isMarkdown ? buildMarkdownSections(documentText ?? "") : []),
		[documentText, isMarkdown],
	);
	const selectedSection =
		markdownSections.find((section) => section.id === activeSectionId) ??
		markdownSections[0];
	const displayedMarkdown =
		mode === "edit"
			? (documentText ?? "")
			: (selectedSection?.markdown ?? documentText ?? "");
	const saveMessage = saveStateMessage(saveState);

	React.useEffect(() => {
		if (!frame) return;
		let acceptResult = true;
		loadResourceText(frame)
			.then((loaded) => {
				if (!acceptResult) return;
				setResourceName(loaded.resourceName);
				setDocumentText(loaded.text);
			})
			.catch((reason: unknown) => {
				if (acceptResult) {
					setLoadError(
						reason instanceof Error ? reason.message : "Unable to load text",
					);
				}
			});
		return () => {
			acceptResult = false;
		};
	}, [frame]);

	React.useEffect(() => {
		document.title = resourceName;
	}, [resourceName]);

	React.useEffect(() => {
		if (
			isMarkdown &&
			!markdownSections.some((section) => section.id === activeSectionId)
		) {
			setActiveSectionId(ALL_MARKDOWN_SECTION_ID);
		}
	}, [activeSectionId, isMarkdown, markdownSections]);

	function selectMarkdownSection(sectionId: string) {
		setActiveSectionId(sectionId);
		contentScrollerRef.current?.scrollTo({ top: 0, behavior: "smooth" });
	}

	async function saveDocument() {
		if (!frame || documentText === null) {
			setSaveState({ status: "error", message: "Missing save target" });
			return;
		}

		setSaveState({ status: "saving" });
		try {
			await saveResourceText(documentText);
			setSaveState({ status: "saved" });
		} catch (error) {
			setSaveState({
				status: "error",
				message: error instanceof Error ? error.message : "Save failed",
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

	if (documentText === null) {
		return (
			<StatusScreen
				tone="loading"
				title="Opening text"
				detail="Loading document content"
			/>
		);
	}

	const appClasses = [
		"app",
		mode === "edit" ? "editing" : "reading",
		documentFormat,
		sidebarOpen ? "sidebar-visible" : "",
	]
		.filter(Boolean)
		.join(" ");

	return (
		<div className={appClasses}>
			<header className="toolbar">
				{mode === "read" && isMarkdown ? (
					<button
						className="icon-button"
						type="button"
						title={sidebarOpen ? "Hide headings" : "Show headings"}
						aria-label={sidebarOpen ? "Hide headings" : "Show headings"}
						onClick={() => setSidebarOpen((open) => !open)}
					>
						{sidebarOpen ? (
							<PanelLeftClose size={18} />
						) : (
							<PanelLeftOpen size={18} />
						)}
					</button>
				) : mode === "edit" ? (
					<div className="toolbar-mode" title="Edit mode" aria-hidden="true">
						<Edit3 size={18} />
					</div>
				) : (
					<div
						className="toolbar-mode"
						title={`${documentFormatLabel(documentFormat)} reader`}
						aria-hidden="true"
					>
						<FileText size={18} />
					</div>
				)}
				<div className="toolbar-title">
					<FileText size={18} />
					<span>{resourceName}</span>
				</div>
				{mode === "edit" ? (
					<div className="save-controls">
						{saveMessage && (
							<span className={`save-status ${saveState.status}`}>
								{saveState.status === "saved" && <Check size={14} />}
								{saveMessage}
							</span>
						)}
						<button
							className="primary-button"
							type="button"
							disabled={saveState.status === "saving"}
							onClick={() => void saveDocument()}
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
						{markdownSections.map((section) => (
							<button
								className={`heading-link ${section.all ? "all" : `depth-${Math.min(section.level, 6)}`} ${
									selectedSection?.id === section.id ? "active" : ""
								}`}
								type="button"
								key={section.id}
								onClick={() => selectMarkdownSection(section.id)}
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
					aria-label={`${documentFormatLabel(documentFormat)} source`}
				>
					<textarea
						spellCheck={false}
						value={documentText}
						onChange={(event) => {
							setDocumentText(event.target.value);
							if (saveState.status !== "idle") {
								setSaveState({ status: "idle" });
							}
						}}
					/>
				</section>
			)}

			{(mode === "read" || isMarkdown || isMermaid) && (
				<main className="content-scroll" ref={contentScrollerRef}>
					{isMarkdown ? (
						<MarkdownDocument source={displayedMarkdown} />
					) : isMermaid ? (
						<div className="mermaid-body">
							<MermaidPreview
								source={documentText}
								debounceMilliseconds={mode === "edit" ? 250 : 0}
							/>
						</div>
					) : (
						<pre className="plain-text-body">{documentText}</pre>
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

function documentFormatLabel(format: TextDocumentFormat): string {
	switch (format) {
		case "markdown":
			return "Markdown";
		case "mermaid":
			return "Mermaid";
		case "plain":
			return "Plain text";
	}
}

function saveStateMessage(state: SaveState): string {
	switch (state.status) {
		case "idle":
			return "";
		case "saving":
			return "Saving";
		case "saved":
			return "Saved";
		case "error":
			return state.message;
	}
}
