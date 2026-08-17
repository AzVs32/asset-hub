import {
  useEffect,
  useRef,
  useState,
  type ChangeEvent,
  type CSSProperties,
  type KeyboardEvent,
} from "react";
import {
  connectAssetHubDirectoryFrame,
  mountAssetHubResourceFrame,
  type AssetHubDirectoryFrameClient,
  type ResourceActionOutput,
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
};

type LibraryModel = {
  mode: "library";
  directory: DirectorySummary;
  games: GameSummary[];
};

type GameModel = {
  mode: "game";
  directory: DirectorySummary;
  documents: GameDocument[];
  cover: GameCoverData | null;
};

type GameDocument = {
  id: string;
  name: "README.md" | "METADATA.yml";
};

type GameCoverData = {
  mime_type: string;
  data: string;
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
    return <GameDetail model={model} client={client} />;
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
                  <h2>{game.name}</h2>
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

function GameDetail({
  model,
  client,
}: {
  model: GameModel;
  client: AssetHubDirectoryFrameClient;
}) {
  const [editing, setEditing] = useState<string | null>(null);
  const [editError, setEditError] = useState<string | null>(null);
  const [documentOutput, setDocumentOutput] = useState<ResourceActionOutput | null>(null);
  const [viewError, setViewError] = useState<string | null>(null);
  const readme = model.documents.find((document) => document.name === "README.md");
  const coverSource = model.cover
    ? `data:${model.cover.mime_type};base64,${model.cover.data}`
    : null;

  useEffect(() => {
    if (!readme) {
      setDocumentOutput(null);
      setViewError("README.md was not found for this game.");
      return;
    }
    let active = true;
    setDocumentOutput(null);
    setViewError(null);
    void client.viewResource(readme.id)
      .then((output) => {
        if (!active) return;
        if (output.view?.view !== "plugin_frame") {
          throw new Error("Resource Text did not return its reader frame.");
        }
        setDocumentOutput(output);
      })
      .catch((reason: unknown) => {
        if (active) setViewError(errorMessage(reason, "Unable to display README.md"));
      });
    return () => {
      active = false;
    };
  }, [client, readme]);

  async function edit(document: GameDocument) {
    setEditing(document.id);
    setEditError(null);
    try {
      await client.editResource(document.id);
    } catch (reason) {
      setEditError(errorMessage(reason, `Unable to edit ${document.name}`));
    } finally {
      setEditing(null);
    }
  }

  return (
    <main className="game-detail">
      <div className="detail-rail">
        <div className={`detail-cover${coverSource ? " has-image" : ""}`}>
          {coverSource ? <img src={coverSource} alt="" /> : <DefaultGameIcon />}
        </div>
        <span className="detail-name">{model.directory.name}</span>
        <small>GAME DOCUMENT</small>
        <div className="detail-actions">
          {model.documents.map((document) => (
            <button
              type="button"
              disabled={editing !== null}
              key={document.id}
              onClick={() => void edit(document)}
            >
              {editing === document.id ? "Opening…" : `Edit ${document.name}`}
            </button>
          ))}
        </div>
        {editError ? <p className="rail-error" role="alert">{editError}</p> : null}
      </div>
      <article className="document">
        {viewError ? (
          <DocumentState tone="error" title="Unable to display README.md" detail={viewError} />
        ) : documentOutput && readme ? (
          <ResourceProviderFrame
            client={client}
            resourceId={readme.id}
            output={documentOutput}
            onError={setViewError}
          />
        ) : (
          <DocumentState tone="loading" title="Opening README.md" detail="Loading text view" />
        )}
      </article>
    </main>
  );
}

function ResourceProviderFrame({
  client,
  resourceId,
  output,
  onError,
}: {
  client: AssetHubDirectoryFrameClient;
  resourceId: string;
  output: ResourceActionOutput;
  onError: (message: string) => void;
}) {
  const frameRef = useRef<HTMLIFrameElement | null>(null);

  useEffect(() => {
    const frame = frameRef.current;
    if (!frame) return;
    let active = true;
    try {
      const mount = mountAssetHubResourceFrame({ client, frame, resourceId, output });
      void mount.ready.catch((reason: unknown) => {
        if (active) onError(errorMessage(reason, "Unable to connect the Resource Text reader"));
      });
      return () => {
        active = false;
        mount.disconnect();
      };
    } catch (reason) {
      onError(errorMessage(reason, "Unable to mount the Resource Text reader"));
    }
  }, [client, onError, output, resourceId]);

  return (
    <iframe
      ref={frameRef}
      className="provider-document"
      sandbox="allow-scripts"
      title="README.md"
    />
  );
}

function DocumentState({
  tone,
  title,
  detail,
}: {
  tone: "loading" | "error";
  title: string;
  detail: string;
}) {
  return (
    <div className={`document-state ${tone}`} role={tone === "error" ? "alert" : "status"}>
      <strong>{title}</strong>
      <span>{detail}</span>
    </div>
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
    return Array.isArray(value.documents)
      && value.documents.every(isGameDocument)
      && isGameCover(value.cover);
  }
  return value.mode === "library"
    && Array.isArray(value.games)
    && value.games.every(isGameSummary);
}

function isGameDocument(value: unknown): value is GameDocument {
  return isObject(value)
    && typeof value.id === "string"
    && (value.name === "README.md" || value.name === "METADATA.yml");
}

function isDirectory(value: unknown): value is DirectorySummary {
  return isObject(value) && typeof value.name === "string" && typeof value.path === "string";
}

function isGameSummary(value: unknown): value is GameSummary {
  return isObject(value)
    && typeof value.id === "string"
    && typeof value.name === "string"
    && typeof value.path === "string";
}

function isCoverResponse(value: unknown): value is {
  cover: GameCoverData | null;
} {
  if (!isObject(value) || !("cover" in value)) return false;
  return isGameCover(value.cover);
}

function isGameCover(value: unknown): value is GameCoverData | null {
  if (value === null) return true;
  return isObject(value)
    && typeof value.mime_type === "string"
    && ICON_TYPES.has(value.mime_type)
    && typeof value.data === "string";
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
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
