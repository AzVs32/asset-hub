import MarkdownIt from "markdown-it";
import Token from "markdown-it/lib/token.mjs";

type Payload = {
  mode?: "read" | "edit";
  resource?: {
    id?: string;
    name?: string;
  };
  save_action?: string;
  markdown?: string;
};

type Heading = {
  id: string;
  level: number;
  title: string;
  tokenIndex: number;
  line: number;
};

type Section = Heading & {
  markdown: string;
  all?: boolean;
};

const markdown = new MarkdownIt({
  html: false,
  linkify: true,
  typographer: true,
});

const payload = readPayload();
const source = payload.markdown ?? "";
const sections = splitSections(source);
const selectedId = "all";

document.title = payload.resource?.name || "Markdown";
if (payload.mode === "edit") {
  renderEditor(source);
} else {
  renderShell(sections, selectedId);
}

function readPayload(): Payload {
  const params = new URLSearchParams(window.location.hash.replace(/^#/, ""));
  const encoded = params.get("payload");
  if (!encoded) return {};
  const base64 = encoded.replace(/-/g, "+").replace(/_/g, "/");
  const padded = base64 + "=".repeat((4 - (base64.length % 4)) % 4);
  const binary = atob(padded);
  const bytes = Uint8Array.from(binary, (char) => char.charCodeAt(0));
  return JSON.parse(new TextDecoder().decode(bytes)) as Payload;
}

function splitSections(markdownSource: string): Section[] {
  const lines = markdownSource.replace(/\r\n?/g, "\n").split("\n");
  const headings = collectHeadings(markdownSource);
  const allSection: Section = {
    id: "all",
    level: 0,
    title: "全部内容",
    tokenIndex: 0,
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
      const next = headings.slice(index + 1).find((item) => item.level <= heading.level);
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
  const tokens = markdown.parse(markdownSource, {});
  const headings: Heading[] = [];
  const usedIds = new Map<string, number>([["all", 1]]);

  for (let index = 0; index < tokens.length; index += 1) {
    const token = tokens[index];
    if (token.type !== "heading_open") continue;
    const inline = tokens[index + 1];
    const level = headingLevel(token);
    const title = plainText(inline);
    if (!title) continue;
    const baseId = slugify(title) || `heading-${headings.length + 1}`;
    const count = usedIds.get(baseId) ?? 0;
    usedIds.set(baseId, count + 1);
    headings.push({
      id: count === 0 ? baseId : `${baseId}-${count + 1}`,
      level,
      title,
      tokenIndex: index,
      line: token.map?.[0] ?? 0,
    });
  }

  return headings;
}

function headingLevel(token: Token): number {
  const level = Number(token.tag.replace(/^h/, ""));
  return Number.isFinite(level) ? level : 1;
}

function plainText(token: Token | undefined): string {
  if (!token) return "";
  if (token.children?.length) {
    return token.children.map((child) => child.content).join("").trim();
  }
  return token.content.trim();
}

function renderShell(items: Section[], activeId: string): void {
  const nav = document.getElementById("title-tree");
  const content = document.getElementById("content");
  if (!nav || !content) return;

  nav.replaceChildren(
    ...items.map((item) => {
      const button = document.createElement("button");
      button.type = "button";
      button.className = item.all ? "tree-item all-item" : `tree-item depth-${Math.min(item.level, 6)}`;
      button.dataset.sectionId = item.id;
      button.textContent = item.title;
      button.addEventListener("click", () => selectSection(items, item.id));
      return button;
    }),
  );

  selectSection(items, activeId);
}

function renderEditor(initialMarkdown: string): void {
  const toolbar = document.getElementById("editor-toolbar");
  const title = document.getElementById("editor-title");
  const nav = document.getElementById("title-tree");
  const editorPane = document.getElementById("editor-pane");
  const editor = document.getElementById("markdown-editor") as HTMLTextAreaElement | null;
  const saveButton = document.getElementById("save-button") as HTMLButtonElement | null;
  if (!toolbar || !title || !nav || !editorPane || !editor || !saveButton) return;

  toolbar.hidden = false;
  editorPane.hidden = false;
  nav.remove();
  title.textContent = payload.resource?.name || "Markdown";
  editor.value = initialMarkdown;
  renderEditorPreview(editor.value);

  editor.addEventListener("input", () => renderEditorPreview(editor.value));
  saveButton.addEventListener("click", () => saveMarkdown(editor.value));
}

function renderEditorPreview(markdownSource: string): void {
  const content = document.getElementById("content");
  if (!content) return;
  content.innerHTML = markdown.render(markdownSource);
}

async function saveMarkdown(markdownSource: string): Promise<void> {
  const resourceId = payload.resource?.id;
  const action = payload.save_action;
  if (!resourceId || !action) return;

  setSaveStatus("Saving");
  try {
    const result = await executeResourceAction(resourceId, action, {
      markdown: markdownSource,
    });
    if (!result.ok) {
      throw new Error(result.error || "Save failed");
    }
    setSaveStatus("Saved");
  } catch (error) {
    setSaveStatus(error instanceof Error ? error.message : "Save failed");
  }
}

function setSaveStatus(value: string): void {
  const status = document.getElementById("save-status");
  if (status) status.textContent = value;
}

function executeResourceAction(
  resourceId: string,
  action: string,
  input: Record<string, unknown>,
): Promise<{ ok: boolean; error?: string | null }> {
  const requestId = requestIdValue();
  return new Promise((resolve) => {
    const timeout = window.setTimeout(() => {
      window.removeEventListener("message", onMessage);
      resolve({ ok: false, error: "Save timed out" });
    }, 30000);

    function onMessage(event: MessageEvent) {
      const message = event.data;
      if (
        !message ||
        message.type !== "asset-hub:execute-resource-action-result" ||
        message.request_id !== requestId
      ) {
        return;
      }
      window.clearTimeout(timeout);
      window.removeEventListener("message", onMessage);
      resolve({ ok: Boolean(message.ok), error: message.error });
    }

    window.addEventListener("message", onMessage);
    window.parent.postMessage(
      {
        type: "asset-hub:execute-resource-action",
        request_id: requestId,
        resource_id: resourceId,
        action,
        input,
      },
      "*",
    );
  });
}

function requestIdValue(): string {
  return globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random()}`;
}

function selectSection(items: Section[], id: string): void {
  const content = document.getElementById("content");
  if (!content) return;
  const selected = items.find((item) => item.id === id) ?? items[0];
  if (!selected) return;

  for (const button of document.querySelectorAll<HTMLButtonElement>(".tree-item")) {
    button.classList.toggle("active", button.dataset.sectionId === selected.id);
  }

  content.innerHTML = markdown.render(selected.markdown);
}

function slugify(value: string): string {
  return value
    .toLowerCase()
    .trim()
    .replace(/[^\p{Letter}\p{Number}\s-]/gu, "")
    .replace(/\s+/g, "-")
    .replace(/-+/g, "-");
}
