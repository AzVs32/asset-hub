import React from "react";

let mermaidModulePromise: Promise<
	(typeof import("mermaid"))["default"]
> | null = null;
let isMermaidInitialized = false;
let nextDiagramId = 0;
let mermaidRenderQueue: Promise<void> = Promise.resolve();

async function loadMermaid() {
	mermaidModulePromise ??= import("mermaid").then((module) => module.default);
	const mermaid = await mermaidModulePromise;
	if (!isMermaidInitialized) {
		mermaid.initialize({
			startOnLoad: false,
			securityLevel: "strict",
		});
		isMermaidInitialized = true;
	}
	return mermaid;
}

function renderMermaidSvg(source: string): Promise<string> {
	const renderOperation = mermaidRenderQueue.then(async () => {
		if (!source.trim()) throw new Error("Diagram source is empty");
		const mermaid = await loadMermaid();
		const id = `asset-hub-mermaid-${++nextDiagramId}`;
		return (await mermaid.render(id, source)).svg;
	});
	mermaidRenderQueue = renderOperation.then(
		() => undefined,
		() => undefined,
	);
	return renderOperation;
}

function renderMermaidInto(container: HTMLElement, source: string): () => void {
	let acceptResult = true;
	if (!container.hasChildNodes()) {
		const status = document.createElement("span");
		status.className = "mermaid-status";
		status.textContent = "Rendering diagram…";
		container.append(status);
	}

	void renderMermaidSvg(source)
		.then((svg) => {
			if (acceptResult) commitMermaidSvg(container, svg);
		})
		.catch((reason: unknown) => {
			if (acceptResult) showMermaidError(container, source, reason);
		});

	return () => {
		acceptResult = false;
	};
}

function commitMermaidSvg(container: HTMLElement, svg: string) {
	// Mermaid's strict security mode owns sanitizing its generated SVG.
	container.innerHTML = svg;
	const rendered = container.querySelector("svg");
	rendered?.setAttribute("role", "img");
	rendered?.setAttribute("aria-label", "Mermaid diagram");
}

function showMermaidError(
	container: HTMLElement,
	source: string,
	reason: unknown,
) {
	container.replaceChildren();
	const message = document.createElement("figcaption");
	message.className = "mermaid-error";
	message.textContent =
		reason instanceof Error ? reason.message : "Unable to render diagram";
	const sourceView = document.createElement("pre");
	sourceView.className = "mermaid-source";
	sourceView.textContent = source;
	container.append(message, sourceView);
}

export function renderEmbeddedMermaidDiagrams(root: HTMLElement): () => void {
	const cancellations = Array.from(
		root.querySelectorAll<HTMLElement>("[data-mermaid-diagram]"),
	).map((container) => {
		const source =
			container.querySelector<HTMLElement>("[data-mermaid-source]")
				?.textContent ?? "";
		return renderMermaidInto(container, source);
	});
	return () => {
		for (const cancel of cancellations) cancel();
	};
}

export function MermaidPreview({
	source,
	debounceMilliseconds = 0,
}: {
	source: string;
	debounceMilliseconds?: number;
}) {
	const previewRef = React.useRef<HTMLElement>(null);

	React.useEffect(() => {
		const container = previewRef.current;
		if (!container) return;
		let cancelRendering = () => {};
		let acceptTimer = true;
		const timer = window.setTimeout(() => {
			if (acceptTimer) cancelRendering = renderMermaidInto(container, source);
		}, debounceMilliseconds);
		return () => {
			acceptTimer = false;
			window.clearTimeout(timer);
			cancelRendering();
		};
	}, [debounceMilliseconds, source]);

	return (
		<figure
			className="mermaid-diagram standalone-mermaid"
			ref={previewRef}
			aria-live="polite"
		/>
	);
}
