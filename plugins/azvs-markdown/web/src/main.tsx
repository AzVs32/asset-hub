import React from "react";
import { createRoot } from "react-dom/client";
import MarkdownIt from "markdown-it";
import {
  Check,
  Edit3,
  Eye,
  FileText,
  Menu,
  Save,
  X,
} from "lucide-react";
import "./styles.css";

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

type ActionResultMessage = {
  type: "asset-hub:execute-resource-action-result";
  request_id: string;
  ok: boolean;
  error?: string | null;
};

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

function App() {
  const [payload] = React.useState<Payload>(initialPayload);
  const [source, setSource] = React.useState(payload.markdown ?? "");
  const [activeId, setActiveId] = React.useState("all");
  const [sidebarOpen, setSidebarOpen] = React.useState(payload.mode !== "edit");
  const [saveState, setSaveState] = React.useState<SaveState>({ status: "idle", label: "" });
  const contentRef = React.useRef<HTMLElement | null>(null);
  const mode = payload.mode === "edit" ? "edit" : "read";
  const title = payload.resource?.name || "Markdown";
  const sections = React.useMemo(() => splitSections(source), [source]);
  const activeSection = sections.find((section) => section.id === activeId) ?? sections[0];
  const renderedHtml = React.useMemo(
    () => renderer.render(mode === "edit" ? source : activeSection?.markdown ?? source),
    [activeSection, mode, source],
  );

  React.useEffect(() => {
    document.title = title;
  }, [title]);

  React.useEffect(() => {
    if (!sections.some((section) => section.id === activeId)) {
      setActiveId("all");
    }
  }, [activeId, sections]);

  function selectSection(id: string) {
    setActiveId(id);
    setSidebarOpen(false);
    contentRef.current?.scrollTo({ top: 0, behavior: "smooth" });
  }

  async function saveMarkdown() {
    const resourceId = payload.resource?.id;
    const action = payload.save_action;
    if (!resourceId || !action) {
      setSaveState({ status: "error", label: "Missing save target" });
      return;
    }

    setSaveState({ status: "saving", label: "Saving" });
    try {
      const result = await executeResourceAction(resourceId, action, { markdown: source });
      if (!result.ok) {
        throw new Error(result.error || "Save failed");
      }
      setSaveState({ status: "saved", label: "Saved" });
    } catch (error) {
      setSaveState({
        status: "error",
        label: error instanceof Error ? error.message : "Save failed",
      });
    }
  }

  return (
    <div className={`app ${mode === "edit" ? "editing" : "reading"} ${sidebarOpen ? "sidebar-visible" : ""}`}>
      <header className="toolbar">
        {mode === "read" ? (
          <button
            className="icon-button"
            type="button"
            title={sidebarOpen ? "Hide headings" : "Show headings"}
            aria-label={sidebarOpen ? "Hide headings" : "Show headings"}
            onClick={() => setSidebarOpen((open) => !open)}
          >
            {sidebarOpen ? <X size={18} /> : <Menu size={18} />}
          </button>
        ) : (
          <div className="toolbar-mode" title="Edit mode" aria-label="Edit mode">
            <Edit3 size={18} />
          </div>
        )}
        <div className="toolbar-title">
          <FileText size={18} />
          <span>{title}</span>
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
              onClick={() => void saveMarkdown()}
            >
              <Save size={16} />
              Save
            </button>
          </div>
        ) : (
          <div className="toolbar-mode" title="Read mode" aria-label="Read mode">
            <Eye size={18} />
          </div>
        )}
      </header>

      {mode === "read" && (
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
        <section className="editor-pane" aria-label="Markdown source">
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

      <main className="content-scroll" ref={contentRef}>
        <article className="markdown-body" dangerouslySetInnerHTML={{ __html: renderedHtml }} />
      </main>
    </div>
  );
}

function readPayload(): Payload {
  try {
    const params = new URLSearchParams(window.location.hash.replace(/^#/, ""));
    const encoded = params.get("payload");
    if (!encoded) return {};
    const base64 = encoded.replace(/-/g, "+").replace(/_/g, "/");
    const padded = base64 + "=".repeat((4 - (base64.length % 4)) % 4);
    const bytes = Uint8Array.from(atob(padded), (character) => character.charCodeAt(0));
    return JSON.parse(new TextDecoder().decode(bytes)) as Payload;
  } catch {
    return {};
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
    return token.children.map((child) => child.content).join("").trim();
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
  resourceId: string,
  action: string,
  input: Record<string, unknown>,
): Promise<{ ok: boolean; error?: string | null }> {
  const requestId = globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random()}`;
  return new Promise((resolve) => {
    const timeout = window.setTimeout(() => {
      window.removeEventListener("message", onMessage);
      resolve({ ok: false, error: "Save timed out" });
    }, 30000);

    function onMessage(event: MessageEvent<ActionResultMessage>) {
      if (event.source !== window.parent) return;
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

createRoot(document.getElementById("root")!).render(<App />);
