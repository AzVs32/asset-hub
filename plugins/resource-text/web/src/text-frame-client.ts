import {
	type AssetHubResourceFrameClient,
	connectAssetHubResourceFrame,
	type JsonObject,
	PLUGIN_API_VERSION,
	type ResourceActionOutput,
} from "@asset-hub/asset-web-sdk";

export type TextDocumentFormat = "markdown" | "mermaid" | "plain";

export type TextFrameContext = {
	mode: "read" | "edit";
	action: string;
	format: TextDocumentFormat;
};

type TextFramePayload = {
	plugin_api: string;
	mode: "read" | "edit";
	action: string;
	format: TextDocumentFormat;
};

type TextLoadResponseBase = {
	protocol: 1;
	resource_name: string;
	byte_length: number;
};

type TextLoadResponse = TextLoadResponseBase &
	(
		| { transfer: "complete"; text: string }
		| { transfer: "chunked"; chunk_size: number }
	);

type TextChunkResponse = {
	protocol: 1;
	offset: number;
	byte_length: number;
	data: string;
	done: boolean;
};

export type LoadedResourceText = {
	resourceName: string;
	text: string;
};

const MAX_ACCEPTED_TEXT_BYTES = 128 * 1024 * 1024;
const MAX_ACCEPTED_CHUNK_BYTES = 4 * 1024 * 1024;

export function readTextFrameContext(): TextFrameContext | null {
	try {
		const params = new URLSearchParams(window.location.hash.replace(/^#/, ""));
		const encoded = params.get("payload");
		if (!encoded) return null;
		const base64 = encoded.replace(/-/g, "+").replace(/_/g, "/");
		const padded = base64 + "=".repeat((4 - (base64.length % 4)) % 4);
		const bytes = Uint8Array.from(atob(padded), (character) =>
			character.charCodeAt(0),
		);
		const payload = JSON.parse(
			new TextDecoder().decode(bytes),
		) as Partial<TextFramePayload>;
		if (
			payload.plugin_api !== PLUGIN_API_VERSION ||
			(payload.mode !== "read" && payload.mode !== "edit") ||
			typeof payload.action !== "string" ||
			!isTextDocumentFormat(payload.format)
		)
			return null;
		return {
			mode: payload.mode,
			action: payload.action,
			format: payload.format,
		};
	} catch {
		return null;
	}
}

export async function loadResourceText(
	frame: TextFrameContext,
): Promise<LoadedResourceText> {
	const load = await executeTextAction(frame, { operation: "load" });
	const description = requireJsonViewData(
		load,
		isTextLoadResponse,
		"Text plugin returned an invalid load response",
	);
	if (description.transfer === "complete") {
		return {
			resourceName: description.resource_name,
			text: removeUtf8Bom(description.text),
		};
	}

	const bytes = new Uint8Array(description.byte_length);
	let offset = 0;
	while (offset < description.byte_length) {
		const result = await executeTextAction(frame, {
			operation: "chunk",
			offset,
		});
		const chunk = requireJsonViewData(
			result,
			isTextChunkResponse,
			"Text plugin returned an invalid chunk",
		);
		if (
			chunk.offset !== offset ||
			chunk.byte_length !== description.byte_length
		) {
			throw new Error("Text chunk sequence does not match the document");
		}
		const chunkBytes = decodeBase64Bytes(chunk.data);
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
		text: removeUtf8Bom(text),
	};
}

export function saveResourceText(text: string): Promise<void> {
	return frameHost().then((host) => host.replaceResourceText(text));
}

function isTextDocumentFormat(value: unknown): value is TextDocumentFormat {
	return value === "markdown" || value === "mermaid" || value === "plain";
}

function requireJsonViewData<T>(
	result: ResourceActionOutput,
	validate: (value: unknown) => value is T,
	invalidMessage: string,
): T {
	const view = result.view;
	if (view?.type !== "json" || !validate(view.data)) {
		throw new Error(invalidMessage);
	}
	return view.data;
}

function isTextLoadResponse(value: unknown): value is TextLoadResponse {
	if (!value || typeof value !== "object") return false;
	const response = value as Partial<TextLoadResponseBase> & {
		transfer?: unknown;
		text?: unknown;
		chunk_size?: unknown;
	};
	if (
		response.protocol !== 1 ||
		typeof response.resource_name !== "string" ||
		!isAcceptedTextByteLength(response.byte_length)
	)
		return false;
	if (response.transfer === "complete")
		return typeof response.text === "string";
	return (
		response.transfer === "chunked" &&
		typeof response.chunk_size === "number" &&
		Number.isSafeInteger(response.chunk_size) &&
		response.chunk_size > 0 &&
		response.chunk_size <= MAX_ACCEPTED_CHUNK_BYTES
	);
}

function isTextChunkResponse(value: unknown): value is TextChunkResponse {
	if (!value || typeof value !== "object") return false;
	const response = value as Partial<TextChunkResponse>;
	return (
		response.protocol === 1 &&
		typeof response.offset === "number" &&
		Number.isSafeInteger(response.offset) &&
		response.offset >= 0 &&
		isAcceptedTextByteLength(response.byte_length) &&
		typeof response.data === "string" &&
		typeof response.done === "boolean"
	);
}

function isAcceptedTextByteLength(value: unknown): value is number {
	return (
		Number.isSafeInteger(value) &&
		(value as number) >= 0 &&
		(value as number) <= MAX_ACCEPTED_TEXT_BYTES
	);
}

function decodeBase64Bytes(value: string): Uint8Array {
	try {
		return Uint8Array.from(atob(value), (character) => character.charCodeAt(0));
	} catch {
		throw new Error("Text chunk is not valid Base64");
	}
}

function removeUtf8Bom(text: string): string {
	return text.replace(/^\uFEFF/, "");
}

function executeTextAction(
	frame: TextFrameContext,
	input: JsonObject,
): Promise<ResourceActionOutput> {
	return frameHost().then((host) =>
		host.executeResourceAction(frame.action, input),
	);
}

let frameHostPromise: Promise<AssetHubResourceFrameClient> | null = null;

function frameHost(): Promise<AssetHubResourceFrameClient> {
	frameHostPromise ??= connectAssetHubResourceFrame();
	return frameHostPromise;
}
