import {
  useEffect,
  useRef,
  useState,
  type ChangeEvent,
  type CSSProperties,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import {
  connectAssetHubDirectoryFrame,
  type AssetHubDirectoryFrameClient,
} from "@asset-hub/plugin-web-sdk";
import "./App.css";

const WORKSPACE_ACTION = "directory.games.workspace";
const CREATE_ACTION = "directory.games.create";
const MAX_ICON_SIZE = 1024 * 1024;
const ICON_TYPES = new Set([
  "image/png",
  "image/jpeg",
  "image/webp",
  "image/gif",
  "image/svg+xml",
]);

type DirectorySummary = {
  name: string;
  path: string;
};

type GameSummary = {
  id: string;
  name: string;
  path: string;
  readme: string | null;
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
};

type WorkspaceModel = LibraryModel | GameModel;

type NewGame = {
  name: string;
  aliases: string[];
  icon: {
    mime_type: string;
    data: string;
  } | null;
};

type SelectedIcon = NonNullable<NewGame["icon"]> & {
  filename: string;
  preview: string;
};

type AliasField = {
  id: number;
  value: string;
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

  async function loadGameCover(gameId: string) {
    if (!client) return null;
    const output = await client.executeDirectoryAction(WORKSPACE_ACTION, {
      operation: "cover",
      game_id: gameId,
    });
    if (output.view?.view !== "json" || !isCoverResponse(output.view.data)) {
      throw new Error("Games returned an invalid cover response.");
    }
    return output.view.data.cover
      ? `data:${output.view.data.cover.mime_type};base64,${output.view.data.cover.data}`
      : null;
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
      onLoadCover={loadGameCover}
      onNavigate={(path) => client.navigateToDirectory(path)}
    />
  );
}

function GameLibrary({
  model,
  onCreate,
  onLoadCover,
  onNavigate,
}: {
  model: LibraryModel;
  onCreate: (input: NewGame) => Promise<void>;
  onLoadCover: (gameId: string) => Promise<string | null>;
  onNavigate: (path: string) => Promise<void>;
}) {
  const dialogRef = useRef<HTMLDialogElement | null>(null);
  const nameInputRef = useRef<HTMLInputElement | null>(null);
  const iconInputRef = useRef<HTMLInputElement | null>(null);
  const nextAliasId = useRef(0);
  const [creating, setCreating] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);
  const [aliasFields, setAliasFields] = useState<AliasField[]>([]);
  const [icon, setIcon] = useState<SelectedIcon | null>(null);

  function openDialog() {
    setFormError(null);
    dialogRef.current?.showModal();
  }

  function closeDialog() {
    if (creating) return;
    dialogRef.current?.close();
  }

  function addAlias() {
    setAliasFields((fields) => [
      ...fields,
      { id: nextAliasId.current++, value: "" },
    ]);
  }

  function updateAlias(id: number, value: string) {
    setAliasFields((fields) => fields.map((field) =>
      field.id === id ? { ...field, value } : field,
    ));
  }

  function removeAlias(id: number) {
    setAliasFields((fields) => fields.filter((field) => field.id !== id));
  }

  async function selectIcon(event: ChangeEvent<HTMLInputElement>) {
    const input = event.currentTarget;
    const file = input.files?.[0];
    input.value = "";
    if (!file) return;
    const mimeType = normalizedIconType(file);
    if (!mimeType) {
      setFormError("Choose a PNG, JPEG, WebP, GIF, or SVG image.");
      return;
    }
    if (file.size === 0 || file.size > MAX_ICON_SIZE) {
      setFormError("The game icon must be between 1 byte and 1 MiB.");
      return;
    }
    try {
      const data = bytesToBase64(await file.arrayBuffer());
      setIcon({
        filename: file.name,
        mime_type: mimeType,
        data,
        preview: `data:${mimeType};base64,${data}`,
      });
      setFormError(null);
    } catch (reason) {
      setFormError(errorMessage(reason, "Unable to read the game icon"));
    }
  }

  async function submit() {
    const nameInput = nameInputRef.current;
    if (!nameInput) return;
    if (!nameInput.checkValidity()) {
      nameInput.reportValidity();
      return;
    }
    setCreating(true);
    setFormError(null);
    try {
      await onCreate({
        name: nameInput.value,
        aliases: aliasFields
          .map((field) => field.value.trim())
          .filter(Boolean),
        icon: icon ? { mime_type: icon.mime_type, data: icon.data } : null,
      });
      nameInput.value = "";
      setAliasFields([]);
      setIcon(null);
      dialogRef.current?.close();
    } catch (reason) {
      setFormError(errorMessage(reason, "Unable to create the game"));
    } finally {
      setCreating(false);
    }
  }

  function submitOnEnter(event: KeyboardEvent<HTMLDivElement>) {
    if (
      event.key === "Enter"
      && event.target instanceof HTMLInputElement
      && event.target.type !== "file"
      && !event.nativeEvent.isComposing
    ) {
      event.preventDefault();
      void submit();
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
                <GameCover
                  game={game}
                  onLoad={onLoadCover}
                  onOpen={() => onNavigate(game.path)}
                />
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
            <p>Add the first game. The plugin will create README.md and METADATA.yml.</p>
            <button className="primary" type="button" onClick={openDialog}>Add your first game</button>
          </section>
        )}
      </main>

      <dialog ref={dialogRef} onCancel={(event) => creating && event.preventDefault()}>
        <div
          className="game-form"
          role="form"
          aria-labelledby="add-game-title"
          onKeyDown={submitOnEnter}
        >
          <div className="dialog-head">
            <div><span className="eyebrow">NEW ENTRY</span><h2 id="add-game-title">Add a game</h2></div>
            <button className="icon-button" type="button" aria-label="Close" onClick={closeDialog}>×</button>
          </div>
          <label>
            English name
            <input ref={nameInputRef} required maxLength={255} placeholder="Game Name" autoFocus />
            <span className="field-help">Required · used as the game directory name</span>
          </label>
          <div className="field-group">
            <div className="field-heading">
              <span>Other names</span>
              <span className="optional">Optional</span>
            </div>
            {aliasFields.length > 0 && (
              <div className="alias-list">
                {aliasFields.map((field, index) => (
                  <div className="alias-row" key={field.id}>
                    <input
                      value={field.value}
                      maxLength={255}
                      placeholder="Alias"
                      aria-label={`Alias ${index + 1}`}
                      autoFocus={index === aliasFields.length - 1}
                      onChange={(event) => updateAlias(field.id, event.target.value)}
                    />
                    <button
                      className="remove-alias"
                      type="button"
                      disabled={creating}
                      aria-label={`Remove alias ${index + 1}`}
                      onClick={() => removeAlias(field.id)}
                    >
                      ×
                    </button>
                  </div>
                ))}
              </div>
            )}
            <button
              className="add-alias"
              type="button"
              disabled={creating || aliasFields.length >= 32}
              onClick={addAlias}
            >
              ＋ Add alias
            </button>
            <span className="field-help">Add one field for each alternative name</span>
          </div>
          <div className="field-group">
            <div className="field-heading">
              <span>Game icon</span>
              <span className="optional">Optional</span>
            </div>
            <div className="icon-picker">
              <div className={`icon-preview${icon ? " has-image" : ""}`}>
                {icon ? <img src={icon.preview} alt="Selected game icon" /> : <DefaultGameIcon />}
              </div>
              <div className="icon-picker-copy">
                <strong>{icon?.filename || "Use the default game icon"}</strong>
                <span>PNG, JPEG, WebP, GIF, or SVG · up to 1 MiB</span>
                <div className="icon-picker-actions">
                  <button
                    className="add-alias"
                    type="button"
                    disabled={creating}
                    onClick={() => iconInputRef.current?.click()}
                  >
                    {icon ? "Change icon" : "Upload icon"}
                  </button>
                  {icon && (
                    <button
                      className="remove-icon"
                      type="button"
                      disabled={creating}
                      onClick={() => setIcon(null)}
                    >
                      Remove
                    </button>
                  )}
                </div>
              </div>
              <input
                ref={iconInputRef}
                className="file-input"
                type="file"
                accept=".png,.jpg,.jpeg,.webp,.gif,.svg,image/png,image/jpeg,image/webp,image/gif,image/svg+xml"
                aria-label="Upload game icon"
                disabled={creating}
                onChange={(event) => void selectIcon(event)}
              />
            </div>
          </div>
          <p className="form-error" role="alert">{formError}</p>
          <div className="dialog-actions">
            <button className="secondary" type="button" disabled={creating} onClick={closeDialog}>Cancel</button>
            <button className="primary" type="button" disabled={creating} onClick={() => void submit()}>
              {creating ? "Creating…" : "Create game"}
            </button>
          </div>
        </div>
      </dialog>
    </div>
  );
}

function GameCover({
  game,
  onLoad,
  onOpen,
}: {
  game: GameSummary;
  onLoad: (gameId: string) => Promise<string | null>;
  onOpen: () => Promise<void>;
}) {
  const coverRef = useRef<HTMLButtonElement | null>(null);
  const started = useRef(false);
  const [source, setSource] = useState<string | null>(null);

  useEffect(() => {
    const target = coverRef.current;
    if (!target || started.current) return;
    let active = true;
    const load = () => {
      if (started.current) return;
      started.current = true;
      void onLoad(game.id)
        .then((cover) => {
          if (active) setSource(cover);
        })
        .catch(() => {
          // A missing or unreadable cover deliberately falls back to the default icon.
        });
    };
    if (!("IntersectionObserver" in window)) {
      load();
      return () => { active = false; };
    }
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) {
        observer.disconnect();
        load();
      }
    }, { rootMargin: "160px" });
    observer.observe(target);
    return () => {
      active = false;
      observer.disconnect();
    };
  }, [game.id, onLoad]);

  return (
    <button
      ref={coverRef}
      className={`cover${source ? " has-image" : ""}`}
      type="button"
      aria-label={`Open ${game.name}`}
      onClick={() => void onOpen()}
    >
      {source ? <img src={source} alt="" /> : <DefaultGameIcon />}
    </button>
  );
}

function DefaultGameIcon() {
  return (
    <svg className="default-game-icon" viewBox="0 0 64 48" aria-hidden="true">
      <path d="M20 14h24c7 0 12 5 14 13l2 9c1 5-1 9-5 10-3 1-6-1-9-5l-3-4H21l-3 4c-3 4-6 6-9 5-4-1-6-5-5-10l2-9c2-8 7-13 14-13Z" />
      <path d="M20 23v10M15 28h10" />
      <circle cx="43" cy="25" r="2" />
      <circle cx="49" cy="31" r="2" />
    </svg>
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
    return nullableString(value.readme);
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
    && nullableString(value.readme);
}

function isCoverResponse(value: unknown): value is {
  cover: { mime_type: string; data: string } | null;
} {
  if (!isObject(value) || !("cover" in value)) return false;
  if (value.cover === null) return true;
  return isObject(value.cover)
    && typeof value.cover.mime_type === "string"
    && ICON_TYPES.has(value.cover.mime_type)
    && typeof value.cover.data === "string";
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

function normalizedIconType(file: File) {
  if (ICON_TYPES.has(file.type)) return file.type;
  const extension = file.name.toLowerCase().match(/\.([^.]+)$/)?.[1];
  return ({
    png: "image/png",
    jpg: "image/jpeg",
    jpeg: "image/jpeg",
    webp: "image/webp",
    gif: "image/gif",
    svg: "image/svg+xml",
  } as Record<string, string>)[extension || ""] || null;
}

function bytesToBase64(buffer: ArrayBuffer) {
  const bytes = new Uint8Array(buffer);
  let binary = "";
  for (let offset = 0; offset < bytes.length; offset += 32_768) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + 32_768));
  }
  return btoa(binary);
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
