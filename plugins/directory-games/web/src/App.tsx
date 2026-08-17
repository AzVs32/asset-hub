import {
  useEffect,
  useRef,
  useState,
  type CSSProperties,
  type FormEvent,
  type ReactNode,
} from "react";
import {
  connectAssetHubDirectoryFrame,
  type AssetHubDirectoryFrameClient,
} from "@asset-hub/plugin-web-sdk";
import "./App.css";

const WORKSPACE_ACTION = "directory.games.workspace";
const CREATE_ACTION = "directory.games.create";

type DirectorySummary = {
  name: string;
  path: string;
};

type GameSummary = {
  id: string;
  name: string;
  path: string;
  readme: string | null;
  hash: string | null;
};

type LibraryModel = {
  mode: "library";
  directory: DirectorySummary;
  games: GameSummary[];
};

type GameModel = {
  mode: "game";
  directory: DirectorySummary;
  readme: string | null;
  hash: string | null;
};

type WorkspaceModel = LibraryModel | GameModel;

type NewGame = {
  name: string;
  title: string;
  summary: string;
  version: string;
};

function App() {
  const [client, setClient] = useState<AssetHubDirectoryFrameClient | null>(null);
  const [model, setModel] = useState<WorkspaceModel | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    let connection: AssetHubDirectoryFrameClient | null = null;

    void connectAssetHubDirectoryFrame()
      .then(async (connected) => {
        connection = connected;
        if (!active) {
          connected.disconnect();
          return;
        }
        setClient(connected);
        const loaded = await loadWorkspace(connected);
        if (active) setModel(loaded);
      })
      .catch((reason: unknown) => {
        if (active) setError(errorMessage(reason, "Unable to open Games"));
      });

    return () => {
      active = false;
      connection?.disconnect();
    };
  }, []);

  async function createGame(input: NewGame) {
    if (!client) throw new Error("The Games workspace is not connected.");
    await client.executeDirectoryAction(CREATE_ACTION, input);
    await client.refreshDirectory();
    setModel(await loadWorkspace(client));
  }

  if (error) {
    return <StatusScreen tone="error" title="Unable to open Games" detail={error} />;
  }
  if (!client || !model) {
    return <StatusScreen tone="loading" title="Opening game library" detail="Loading directory data" />;
  }
  if (model.mode === "game") {
    return <GameDetail model={model} />;
  }
  return (
    <GameLibrary
      model={model}
      onCreate={createGame}
      onNavigate={(path) => client.navigateToDirectory(path)}
    />
  );
}

function GameLibrary({
  model,
  onCreate,
  onNavigate,
}: {
  model: LibraryModel;
  onCreate: (input: NewGame) => Promise<void>;
  onNavigate: (path: string) => Promise<void>;
}) {
  const dialogRef = useRef<HTMLDialogElement | null>(null);
  const [creating, setCreating] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);

  function openDialog() {
    setFormError(null);
    dialogRef.current?.showModal();
  }

  function closeDialog() {
    if (creating) return;
    dialogRef.current?.close();
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = event.currentTarget;
    const fields = new FormData(form);
    setCreating(true);
    setFormError(null);
    try {
      await onCreate({
        name: String(fields.get("name") ?? ""),
        title: String(fields.get("title") ?? ""),
        summary: String(fields.get("summary") ?? ""),
        version: String(fields.get("version") ?? ""),
      });
      form.reset();
      dialogRef.current?.close();
    } catch (reason) {
      setFormError(errorMessage(reason, "Unable to create the game"));
    } finally {
      setCreating(false);
    }
  }

  return (
    <div className="workspace-shell">
      <header className="hero">
        <div>
          <span className="eyebrow">GAME LIBRARY</span>
          <h1>{model.directory.name}</h1>
          <p>
            {model.games.length} {model.games.length === 1 ? "game" : "games"} · README-powered library
          </p>
        </div>
        <button className="primary" type="button" onClick={openDialog}>＋ Add game</button>
      </header>

      <main className="library">
        {model.games.length > 0 ? (
          <div className="game-grid">
            {model.games.map((game, index) => (
              <article
                className="game-card"
                style={{ "--delay": `${index * 45}ms` } as CSSProperties}
                key={game.id}
              >
                <button
                  className="cover"
                  type="button"
                  aria-label={`Open ${game.name}`}
                  onClick={() => void onNavigate(game.path)}
                >
                  <span className="cover-mark">G{index + 1}</span>
                </button>
                <div className="card-copy">
                  <span className="folder-name">{game.name}</span>
                  <h2>{readTitle(game.readme, game.name)}</h2>
                  <div className="readme-preview">
                    <MarkdownDocument source={game.readme} compact />
                  </div>
                  <button className="text-button" type="button" onClick={() => void onNavigate(game.path)}>
                    Open game <span>→</span>
                  </button>
                </div>
              </article>
            ))}
          </div>
        ) : (
          <section className="empty-state">
            <div className="empty-icon" aria-hidden="true">🎮</div>
            <h2>Your library is ready</h2>
            <p>Add the first game. The plugin will create its README.md, HASH.md and public directory.</p>
            <button className="primary" type="button" onClick={openDialog}>Add your first game</button>
          </section>
        )}
      </main>

      <dialog ref={dialogRef} onCancel={(event) => creating && event.preventDefault()}>
        <form method="dialog" onSubmit={(event) => void submit(event)}>
          <div className="dialog-head">
            <div><span className="eyebrow">NEW ENTRY</span><h2>Add a game</h2></div>
            <button className="icon-button" type="button" aria-label="Close" onClick={closeDialog}>×</button>
          </div>
          <label>
            Directory name
            <input name="name" required maxLength={64} pattern="[A-Za-z0-9._-]+" placeholder="game_name_1" />
          </label>
          <label>
            Display title
            <input name="title" required maxLength={120} placeholder="Game name" />
          </label>
          <label>
            Summary
            <textarea name="summary" required maxLength={1000} rows={4} placeholder="What makes this game special?" />
          </label>
          <label>
            Version
            <input name="version" maxLength={64} defaultValue="0.1.0" />
          </label>
          <p className="form-error" role="alert">{formError}</p>
          <div className="dialog-actions">
            <button className="secondary" type="button" disabled={creating} onClick={closeDialog}>Cancel</button>
            <button className="primary" type="submit" disabled={creating}>
              {creating ? "Creating…" : "Create game"}
            </button>
          </div>
        </form>
      </dialog>
    </div>
  );
}

function GameDetail({ model }: { model: GameModel }) {
  return (
    <main className="game-detail">
      <div className="detail-rail">
        <div className="game-glyph">G</div>
        <span>{model.directory.name}</span>
        <small>GAME DOCUMENT</small>
      </div>
      <article className="document">
        <div className="document-meta"><span>README.md</span><span>{model.directory.path}</span></div>
        <div className="prose"><MarkdownDocument source={model.readme} /></div>
      </article>
      <aside className="hash-panel">
        <span className="eyebrow">INTEGRITY</span>
        <h2>HASH.md</h2>
        <pre>{model.hash === null ? "No HASH.md was found." : model.hash || "Empty file · awaiting integrity data"}</pre>
      </aside>
    </main>
  );
}

function MarkdownDocument({ source, compact = false }: { source: string | null; compact?: boolean }) {
  const lines = (source || "No README.md was found for this game.").split(/\r?\n/);
  const blocks: ReactNode[] = [];
  let index = 0;

  while (index < lines.length) {
    const line = lines[index];
    if (!line.trim()) {
      index += 1;
      continue;
    }
    const heading = /^(#{1,3})\s+(.+)$/.exec(line);
    if (heading) {
      if (!(compact && heading[1].length === 1)) {
        const content = inlineMarkdown(heading[2]);
        if (heading[1].length === 1) blocks.push(<h1 key={index}>{content}</h1>);
        else if (heading[1].length === 2) blocks.push(<h2 key={index}>{content}</h2>);
        else blocks.push(<h3 key={index}>{content}</h3>);
      }
      index += 1;
      continue;
    }
    if (/^[-*]\s+/.test(line)) {
      const items: ReactNode[] = [];
      const key = index;
      while (index < lines.length && /^[-*]\s+/.test(lines[index])) {
        items.push(<li key={index}>{inlineMarkdown(lines[index].replace(/^[-*]\s+/, ""))}</li>);
        index += 1;
      }
      blocks.push(<ul key={key}>{items}</ul>);
      continue;
    }
    const paragraph: string[] = [];
    const key = index;
    while (index < lines.length && lines[index].trim() && !/^(?:#{1,3}|[-*])\s+/.test(lines[index])) {
      paragraph.push(lines[index]);
      index += 1;
    }
    blocks.push(<p key={key}>{inlineMarkdown(paragraph.join(" "))}</p>);
  }

  return <>{blocks}</>;
}

function inlineMarkdown(value: string): ReactNode[] {
  return value.split(/`([^`]+)`/g).map((part, index) =>
    index % 2 === 1 ? <code key={index}>{part}</code> : part,
  );
}

async function loadWorkspace(client: AssetHubDirectoryFrameClient): Promise<WorkspaceModel> {
  const output = await client.executeDirectoryAction(WORKSPACE_ACTION, { operation: "load" });
  if (output.view?.view !== "json" || !isWorkspaceModel(output.view.data)) {
    throw new Error("Games returned an invalid workspace response.");
  }
  document.title = output.view.data.directory.name || "Games";
  return output.view.data;
}

function isWorkspaceModel(value: unknown): value is WorkspaceModel {
  if (!isObject(value) || !isDirectory(value.directory)) return false;
  if (value.mode === "game") {
    return nullableString(value.readme) && nullableString(value.hash);
  }
  return value.mode === "library"
    && Array.isArray(value.games)
    && value.games.every(isGameSummary);
}

function isDirectory(value: unknown): value is DirectorySummary {
  return isObject(value) && typeof value.name === "string" && typeof value.path === "string";
}

function isGameSummary(value: unknown): value is GameSummary {
  return isObject(value)
    && typeof value.id === "string"
    && typeof value.name === "string"
    && typeof value.path === "string"
    && nullableString(value.readme)
    && nullableString(value.hash);
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function nullableString(value: unknown): value is string | null {
  return value === null || typeof value === "string";
}

function readTitle(readme: string | null, fallback: string) {
  return readme?.match(/^#\s+(.+)$/m)?.[1] || fallback;
}

function errorMessage(reason: unknown, fallback: string) {
  return reason instanceof Error ? reason.message : fallback;
}

function StatusScreen({ tone, title, detail }: { tone: "loading" | "error"; title: string; detail: string }) {
  return (
    <main className={`status-screen ${tone}`}>
      <div aria-hidden="true">🎮</div>
      <h1>{title}</h1>
      <p>{detail}</p>
    </main>
  );
}

export default App;
