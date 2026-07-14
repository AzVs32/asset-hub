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
  resource_id: string;
  resource_name: string;
  action: string;
};

type Chapter = {
  title: string;
  html: string;
};

type Book = {
  title: string;
  author: string | null;
  cover: string | null;
  chapters: Chapter[];
};

type ActionResultMessage = {
  type: "asset-hub:execute-resource-action-result";
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
  const [fontSize, setFontSize] = React.useState(16);
  const [sidebarOpen, setSidebarOpen] = React.useState(true);
  const [error, setError] = React.useState<string | null>(null);
  const readerRef = React.useRef<HTMLElement | null>(null);

  React.useEffect(() => {
    if (!payload) {
      setError("Invalid EPUB frame payload");
      return;
    }
    document.title = payload.resource_name || "EPUB Reader";
    loadBook(payload).then(setBook).catch((reason: unknown) => {
      setError(reason instanceof Error ? reason.message : "Unable to load EPUB");
    });
  }, []);

  const chapters = book?.chapters ?? [];
  const chapter = chapters[activeChapter];

  function selectChapter(index: number) {
    setActiveChapter(index);
    setSidebarOpen(false);
    readerRef.current?.scrollTo({ top: 0, behavior: "smooth" });
  }

  if (error) {
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
          {chapters.map((item, index) => (
            <button
              className={index === activeChapter ? "chapter-link active" : "chapter-link"}
              type="button"
              key={`${index}-${item.title}`}
              onClick={() => selectChapter(index)}
            >
              <span>{String(index + 1).padStart(2, "0")}</span>
              {item.title}
            </button>
          ))}
        </nav>
      </aside>

      <main className="reader-scroll" ref={readerRef}>
        {chapter ? (
          <article className="reader" style={{ "--reader-font-size": `${fontSize}px` } as React.CSSProperties}>
            <div className="chapter-heading">
              <span>Chapter {activeChapter + 1} of {chapters.length}</span>
              <h2>{chapter.title}</h2>
            </div>
            <div className="chapter-content" dangerouslySetInnerHTML={{ __html: chapter.html }} />
            <footer className="chapter-navigation">
              <button
                type="button"
                disabled={activeChapter === 0}
                onClick={() => selectChapter(activeChapter - 1)}
              >
                <ChevronLeft size={18} /> Previous
              </button>
              <button
                type="button"
                disabled={activeChapter >= chapters.length - 1}
                onClick={() => selectChapter(activeChapter + 1)}
              >
                Next <ChevronRight size={18} />
              </button>
            </footer>
          </article>
        ) : (
          <StatusScreen tone="error" title="This EPUB has no readable chapters" detail={book.title} />
        )}
      </main>
    </div>
  );
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
    if (!value.resource_id || !value.resource_name || !value.action) return null;
    return value as FramePayload;
  } catch {
    return null;
  }
}

function loadBook(frame: FramePayload): Promise<Book> {
  const requestId = globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random()}`;
  return new Promise((resolve, reject) => {
    const timeout = window.setTimeout(() => {
      window.removeEventListener("message", onMessage);
      reject(new Error("EPUB loading timed out"));
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
      if (!message.ok) {
        reject(new Error(message.error || "EPUB action failed"));
        return;
      }
      const view = message.data?.view;
      if (view?.view !== "json" || !isBook(view.data)) {
        reject(new Error("EPUB plugin returned an invalid book payload"));
        return;
      }
      resolve(view.data);
    }

    window.addEventListener("message", onMessage);
    window.parent.postMessage(
      {
        type: "asset-hub:execute-resource-action",
        request_id: requestId,
        resource_id: frame.resource_id,
        action: frame.action,
        input: { operation: "load" },
      },
      "*",
    );
  });
}

function isBook(value: unknown): value is Book {
  if (!value || typeof value !== "object") return false;
  const book = value as Partial<Book>;
  return (
    typeof book.title === "string" &&
    (book.author === null || typeof book.author === "string") &&
    (book.cover === null || typeof book.cover === "string") &&
    Array.isArray(book.chapters) &&
    book.chapters.every((chapter) => (
      chapter && typeof chapter.title === "string" && typeof chapter.html === "string"
    ))
  );
}

createRoot(document.getElementById("root")!).render(<App />);
