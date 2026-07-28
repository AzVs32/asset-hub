import React from "react";
import { createRoot } from "react-dom/client";
import {
  BookOpen,
  ChevronLeft,
  ChevronRight,
  Menu,
  Minus,
  Plus,
  X,
} from "lucide-react";
import "./styles.css";

type FramePayload = {
  plugin_api: string;
  resource_id: string;
  resource_name: string;
  action: string;
};

type ChapterSummary = {
  index: number;
  title: string;
};

type ChapterContent = ChapterSummary & {
  html: string;
  styles: string[];
};

type Book = {
  title: string;
  author: string | null;
  cover: string | null;
  chapters: ChapterSummary[];
  initial_chapter: ChapterContent | null;
};

type ActionResultMessage = {
  type: "asset-hub:execute-resource-action-result";
  plugin_api: string;
  request_id: string;
  ok: boolean;
  data?: {
    view?: {
      view?: string;
      data?: unknown;
    };
  };
  error?: string | null;
};

const payload = readPayload();

function App() {
  const [book, setBook] = React.useState<Book | null>(null);
  const [activeChapter, setActiveChapter] = React.useState(0);
  const [chapter, setChapter] = React.useState<ChapterContent | null>(null);
  const [chapterLoading, setChapterLoading] = React.useState(false);
  const [fontSize, setFontSize] = React.useState(18);
  const [sidebarOpen, setSidebarOpen] = React.useState(true);
  const [error, setError] = React.useState<string | null>(null);
  const [pendingAnchor, setPendingAnchor] = React.useState<string | null>(null);
  const readerRef = React.useRef<HTMLElement | null>(null);
  const chapterCache = React.useRef(new Map<number, ChapterContent>());
  const selectionVersion = React.useRef(0);

  React.useEffect(() => {
    if (!payload) {
      setError("Invalid EPUB frame payload");
      return;
    }
    document.title = payload.resource_name || "EPUB Reader";
    executeEpubAction(payload, { operation: "load" }, isBook)
      .then((loaded) => {
        setBook(loaded);
        if (loaded.initial_chapter) {
          chapterCache.current.set(loaded.initial_chapter.index, loaded.initial_chapter);
          setChapter(loaded.initial_chapter);
        } else if (loaded.chapters.length > 0) {
          void selectChapter(0, null, loaded);
        }
      })
      .catch((reason: unknown) => {
        setError(reason instanceof Error ? reason.message : "Unable to load EPUB");
      });
  }, []);

  React.useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.defaultPrevented || event.altKey || event.ctrlKey || event.metaKey) return;
      const target = event.target as HTMLElement | null;
      if (target?.matches("input, textarea, select, [contenteditable=true]")) return;
      if (event.key === "ArrowLeft" && activeChapter > 0) {
        void selectChapter(activeChapter - 1);
      } else if (event.key === "ArrowRight" && book && activeChapter < book.chapters.length - 1) {
        void selectChapter(activeChapter + 1);
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [activeChapter, book]);

  async function selectChapter(index: number, anchor: string | null = null, sourceBook = book) {
    if (!sourceBook || index < 0 || index >= sourceBook.chapters.length || !payload) return;
    const version = ++selectionVersion.current;
    setActiveChapter(index);
    setError(null);
    setPendingAnchor(anchor);
    const cached = chapterCache.current.get(index);
    if (cached) {
      setChapter(cached);
      scrollReader(anchor);
      return;
    }

    setChapter(null);
    setChapterLoading(true);
    readerRef.current?.scrollTo({ top: 0 });
    try {
      const loaded = await executeEpubAction(
        payload,
        { operation: "chapter", index },
        isChapterContent,
      );
      chapterCache.current.set(index, loaded);
      if (selectionVersion.current === version) {
        setChapter(loaded);
      }
    } catch (reason) {
      if (selectionVersion.current === version) {
        setError(reason instanceof Error ? reason.message : "Unable to load chapter");
      }
    } finally {
      if (selectionVersion.current === version) setChapterLoading(false);
    }
  }

  function scrollReader(anchor: string | null) {
    if (!anchor) readerRef.current?.scrollTo({ top: 0, behavior: "smooth" });
  }

  if (error && !book) {
    return <StatusScreen tone="error" title="Unable to open this book" detail={error} />;
  }

  if (!book) {
    return <StatusScreen tone="loading" title="Opening EPUB" detail={payload?.resource_name ?? "Loading book data"} />;
  }

  return (
    <div className={sidebarOpen ? "app sidebar-visible" : "app"}>
      <header className="toolbar">
        <button
          className="icon-button"
          type="button"
          title={sidebarOpen ? "Hide chapters" : "Show chapters"}
          aria-label={sidebarOpen ? "Hide chapters" : "Show chapters"}
          onClick={() => setSidebarOpen((open) => !open)}
        >
          {sidebarOpen ? <X size={18} /> : <Menu size={18} />}
        </button>
        <div className="toolbar-title">
          <BookOpen size={18} />
          <span>{book.title}</span>
        </div>
        <div className="font-controls" aria-label="Reading font size">
          <button
            className="icon-button"
            type="button"
            title="Decrease font size"
            aria-label="Decrease font size"
            disabled={fontSize <= 15}
            onClick={() => setFontSize((size) => Math.max(15, size - 1))}
          >
            <Minus size={17} />
          </button>
          <span>{fontSize}</span>
          <button
            className="icon-button"
            type="button"
            title="Increase font size"
            aria-label="Increase font size"
            disabled={fontSize >= 25}
            onClick={() => setFontSize((size) => Math.min(25, size + 1))}
          >
            <Plus size={17} />
          </button>
        </div>
      </header>

      <aside className="sidebar" aria-label="Book chapters">
        <div className="book-summary">
          {book.cover ? (
            <img className="cover" src={book.cover} alt={`Cover of ${book.title}`} />
          ) : (
            <div className="cover cover-placeholder"><BookOpen size={28} /></div>
          )}
          <h1>{book.title}</h1>
          {book.author && <p>{book.author}</p>}
        </div>
        <nav className="chapter-list">
          {book.chapters.map((item) => (
            <button
              className={item.index === activeChapter ? "chapter-link active" : "chapter-link"}
              type="button"
              key={item.index}
              onClick={() => void selectChapter(item.index)}
            >
              <span>{String(item.index + 1).padStart(2, "0")}</span>
              {item.title}
            </button>
          ))}
        </nav>
      </aside>

      <main className="reader-scroll" ref={readerRef}>
        {book.chapters.length === 0 ? (
          <StatusScreen tone="error" title="This EPUB has no readable chapters" detail={book.title} />
        ) : (
          <article className="reader">
            <div className="chapter-heading">
              <span>Chapter {activeChapter + 1} of {book.chapters.length}</span>
              <h2>{book.chapters[activeChapter]?.title}</h2>
            </div>
            {error ? (
              <div className="chapter-error" role="alert">
                <p>{error}</p>
                <button type="button" onClick={() => void selectChapter(activeChapter)}>Retry</button>
              </div>
            ) : chapterLoading || !chapter ? (
              <div className="chapter-loading" aria-live="polite">Loading chapter</div>
            ) : (
              <ChapterDocument
                chapter={chapter}
                fontSize={fontSize}
                anchor={pendingAnchor}
                onNavigate={(index, anchor) => void selectChapter(index, anchor)}
              />
            )}
            <footer className="chapter-navigation">
              <button
                type="button"
                disabled={activeChapter === 0 || chapterLoading}
                onClick={() => void selectChapter(activeChapter - 1)}
              >
                <ChevronLeft size={18} /> Previous
              </button>
              <button
                type="button"
                disabled={activeChapter >= book.chapters.length - 1 || chapterLoading}
                onClick={() => void selectChapter(activeChapter + 1)}
              >
                Next <ChevronRight size={18} />
              </button>
            </footer>
          </article>
        )}
      </main>
    </div>
  );
}

const chapterBaseStyles = `
  :host {
    display: block;
    contain: layout paint style;
    color: #2a3033;
    font-family: Georgia, "Times New Roman", serif;
    font-size: var(--epub-font-size, 18px);
    line-height: 1.85;
  }
  *, *::before, *::after { box-sizing: border-box; }
  .epub-document { min-width: 0; overflow-wrap: anywhere; }
  p { margin: 0 0 1.05em; }
  h1, h2, h3, h4, h5, h6 { color: #1f282b; line-height: 1.3; }
  img, svg, video { max-width: 100%; height: auto; }
  audio { width: min(100%, 36rem); }
  table { display: block; max-width: 100%; overflow-x: auto; border-collapse: collapse; }
  td, th { padding: 6px 8px; border: 1px solid #d9dedf; }
  a { color: #246a64; text-decoration-thickness: 1px; text-underline-offset: 0.15em; }
  pre { max-width: 100%; overflow-x: auto; white-space: pre-wrap; }
`;

function ChapterDocument({
  chapter,
  fontSize,
  anchor,
  onNavigate,
}: {
  chapter: ChapterContent;
  fontSize: number;
  anchor: string | null;
  onNavigate: (index: number, anchor: string | null) => void;
}) {
  const hostRef = React.useRef<HTMLDivElement | null>(null);
  const navigateRef = React.useRef(onNavigate);
  navigateRef.current = onNavigate;

  React.useLayoutEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const root = host.shadowRoot ?? host.attachShadow({ mode: "open" });
    root.replaceChildren();

    const style = document.createElement("style");
    style.textContent = `${chapterBaseStyles}\n${chapter.styles.join("\n")}`;
    const content = document.createElement("div");
    content.className = "epub-document";
    content.innerHTML = chapter.html;
    root.append(style, content);

    function onClick(event: Event) {
      const target = event.target as Element | null;
      const link = target?.closest("a[href]") as HTMLAnchorElement | null;
      if (!link) return;
      const href = link.getAttribute("href") ?? "";
      if (href.startsWith("#")) {
        event.preventDefault();
        scrollToAnchor(root, href.slice(1));
        return;
      }
      if (href.startsWith("epub://chapter/")) {
        event.preventDefault();
        try {
          const url = new URL(href);
          const index = Number.parseInt(url.pathname.replace(/^\//, ""), 10);
          if (Number.isSafeInteger(index) && index >= 0) {
            navigateRef.current(index, url.hash ? decodeURIComponent(url.hash.slice(1)) : null);
          }
        } catch {
          // Invalid links were already removed by the runtime sanitizer.
        }
        return;
      }
      if (/^(?:https?:|mailto:|tel:)/i.test(href)) {
        event.preventDefault();
      }
    }

    root.addEventListener("click", onClick);
    if (anchor) requestAnimationFrame(() => scrollToAnchor(root, anchor));
    return () => root.removeEventListener("click", onClick);
  }, [chapter, anchor]);

  return (
    <div
      ref={hostRef}
      className="chapter-document"
      style={{ "--epub-font-size": `${fontSize}px` } as React.CSSProperties}
    />
  );
}

function scrollToAnchor(root: ShadowRoot, anchor: string) {
  const target = Array.from(root.querySelectorAll<HTMLElement>("[id], [name]"))
    .find((element) => element.id === anchor || element.getAttribute("name") === anchor);
  target?.scrollIntoView({ block: "start", behavior: "smooth" });
}

function StatusScreen({ tone, title, detail }: { tone: "loading" | "error"; title: string; detail: string }) {
  return (
    <main className={`status-screen ${tone}`}>
      <BookOpen size={32} />
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
    const bytes = Uint8Array.from(atob(padded), (character) => character.charCodeAt(0));
    const value = JSON.parse(new TextDecoder().decode(bytes)) as Partial<FramePayload>;
    if (
      typeof value.plugin_api !== "string"
      || !value.plugin_api
      || !value.resource_id
      || !value.resource_name
      || !value.action
    ) return null;
    return value as FramePayload;
  } catch {
    return null;
  }
}

function executeEpubAction<T>(
  frame: FramePayload,
  input: Record<string, unknown>,
  validate: (value: unknown) => value is T,
): Promise<T> {
  const requestId = globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random()}`;
  return new Promise((resolve, reject) => {
    const timeout = window.setTimeout(() => {
      window.removeEventListener("message", onMessage);
      reject(new Error("EPUB request timed out"));
    }, 30000);

    function onMessage(event: MessageEvent<ActionResultMessage>) {
      if (event.source !== window.parent) return;
      const message = event.data;
      if (
        !message ||
        message.type !== "asset-hub:execute-resource-action-result" ||
        message.plugin_api !== frame.plugin_api ||
        message.request_id !== requestId
      ) return;
      window.clearTimeout(timeout);
      window.removeEventListener("message", onMessage);
      if (!message.ok) {
        reject(new Error(message.error || "EPUB action failed"));
        return;
      }
      const view = message.data?.view;
      if (view?.view !== "json" || !validate(view.data)) {
        reject(new Error("EPUB plugin returned an invalid payload"));
        return;
      }
      resolve(view.data);
    }

    window.addEventListener("message", onMessage);
    window.parent.postMessage(
      {
        type: "asset-hub:execute-resource-action",
        plugin_api: frame.plugin_api,
        request_id: requestId,
        resource_id: frame.resource_id,
        action: frame.action,
        input,
      },
      "*",
    );
  });
}

function isChapterSummary(value: unknown): value is ChapterSummary {
  if (!value || typeof value !== "object") return false;
  const chapter = value as Partial<ChapterSummary>;
  return Number.isSafeInteger(chapter.index) && typeof chapter.title === "string";
}

function isChapterContent(value: unknown): value is ChapterContent {
  if (!isChapterSummary(value)) return false;
  const chapter = value as Partial<ChapterContent>;
  return typeof chapter.html === "string"
    && Array.isArray(chapter.styles)
    && chapter.styles.every((style) => typeof style === "string");
}

function isBook(value: unknown): value is Book {
  if (!value || typeof value !== "object") return false;
  const book = value as Partial<Book>;
  return typeof book.title === "string"
    && (book.author === null || typeof book.author === "string")
    && (book.cover === null || typeof book.cover === "string")
    && Array.isArray(book.chapters)
    && book.chapters.every(isChapterSummary)
    && (book.initial_chapter === null || isChapterContent(book.initial_chapter));
}

createRoot(document.getElementById("root")!).render(<App />);
