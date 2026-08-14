const app = document.querySelector("#app");
let host;
let model;

function escapeHtml(value = "") {
  return String(value).replace(/[&<>'"]/g, (char) => ({
    "&": "&amp;", "<": "&lt;", ">": "&gt;", "'": "&#39;", '"': "&quot;",
  })[char]);
}

function markdown(value) {
  const safe = escapeHtml(value || "No README.md was found for this game.");
  return safe
    .replace(/^### (.+)$/gm, "<h3>$1</h3>")
    .replace(/^## (.+)$/gm, "<h2>$1</h2>")
    .replace(/^# (.+)$/gm, "<h1>$1</h1>")
    .replace(/`([^`]+)`/g, "<code>$1</code>")
    .replace(/^[-*] (.+)$/gm, "<li>$1</li>")
    .replace(/\n\n/g, "</p><p>")
    .replace(/\n/g, "<br />");
}

function readTitle(readme, fallback) {
  return readme?.match(/^#\s+(.+)$/m)?.[1] || fallback;
}

function renderLibrary() {
  const games = model.games || [];
  app.innerHTML = `
    <header class="hero">
      <div>
        <span class="eyebrow">GAME LIBRARY</span>
        <h1>${escapeHtml(model.directory.name)}</h1>
        <p>${games.length} ${games.length === 1 ? "game" : "games"} · README-powered library</p>
      </div>
      <button class="primary" id="add-game">＋ Add game</button>
    </header>
    <main class="library">
      ${games.length ? `<div class="game-grid">${games.map((game, index) => `
        <article class="game-card" style="--delay:${index * 45}ms">
          <button class="cover" data-path="${escapeHtml(game.path)}" aria-label="Open ${escapeHtml(game.name)}">
            <span class="cover-mark">G${index + 1}</span>
          </button>
          <div class="card-copy">
            <span class="folder-name">${escapeHtml(game.name)}</span>
            <h2>${escapeHtml(readTitle(game.readme, game.name))}</h2>
            <div class="readme-preview">${markdown(game.readme)}</div>
            <button class="text-button" data-path="${escapeHtml(game.path)}">Open game <span>→</span></button>
          </div>
        </article>`).join("")}</div>` : `
        <section class="empty-state">
          <div class="empty-icon">🎮</div><h2>Your library is ready</h2>
          <p>Add the first game. The plugin will create its README.md, HASH.md and public directory.</p>
          <button class="primary" id="empty-add">Add your first game</button>
        </section>`}
    </main>
    <dialog id="game-dialog">
      <div id="game-form" role="form">
        <div class="dialog-head"><div><span class="eyebrow">NEW ENTRY</span><h2>Add a game</h2></div><button type="button" class="icon-button" id="close-dialog">×</button></div>
        <label>Directory name<input name="name" required maxlength="64" pattern="[A-Za-z0-9._-]+" placeholder="game_name_1" /></label>
        <label>Display title<input name="title" required maxlength="120" placeholder="Game name" /></label>
        <label>Summary<textarea name="summary" required maxlength="1000" rows="4" placeholder="What makes this game special?"></textarea></label>
        <label>Version<input name="version" maxlength="64" value="0.1.0" /></label>
        <p class="form-error" id="form-error"></p>
        <div class="dialog-actions"><button type="button" class="secondary" id="cancel-dialog">Cancel</button><button type="button" class="primary" id="submit-game">Create game</button></div>
      </div>
    </dialog>`;
  bindLibrary();
}

function renderGame() {
  app.innerHTML = `
    <main class="game-detail">
      <div class="detail-rail"><div class="game-glyph">G</div><span>${escapeHtml(model.directory.name)}</span><small>GAME DOCUMENT</small></div>
      <article class="document"><div class="document-meta"><span>README.md</span><span>${escapeHtml(model.directory.path)}</span></div><div class="prose"><p>${markdown(model.readme)}</p></div></article>
      <aside class="hash-panel"><span class="eyebrow">INTEGRITY</span><h2>HASH.md</h2><pre>${escapeHtml(model.hash === null ? "No HASH.md was found." : model.hash || "Empty file · awaiting integrity data")}</pre></aside>
    </main>`;
}

function bindLibrary() {
  const dialog = document.querySelector("#game-dialog");
  const openDialog = () => dialog.showModal();
  document.querySelector("#add-game")?.addEventListener("click", openDialog);
  document.querySelector("#empty-add")?.addEventListener("click", openDialog);
  document.querySelector("#close-dialog")?.addEventListener("click", () => dialog.close());
  document.querySelector("#cancel-dialog")?.addEventListener("click", () => dialog.close());
  document.querySelectorAll("[data-path]").forEach((button) => button.addEventListener("click", () => host.navigateToDirectory(button.dataset.path)));
  document.querySelector("#submit-game").addEventListener("click", async () => {
    const container = document.querySelector("#game-form");
    const inputs = [...container.querySelectorAll("input, textarea")];
    const invalid = inputs.find((input) => !input.checkValidity());
    if (invalid) {
      invalid.reportValidity();
      return;
    }
    const button = document.querySelector("#submit-game");
    const error = document.querySelector("#form-error");
    button.disabled = true; button.textContent = "Creating…"; error.textContent = "";
    try {
      const fields = Object.fromEntries(inputs.map((input) => [input.name, input.value]));
      await host.executeDirectoryAction("azvs.games.create", fields);
      dialog.close();
      await host.refreshDirectory();
      await load();
    } catch (cause) {
      error.textContent = cause instanceof Error ? cause.message : String(cause);
    } finally {
      button.disabled = false; button.textContent = "Create game";
    }
  });
}

async function load() {
  const output = await host.executeDirectoryAction("azvs.games.workspace", { operation: "load" });
  if (output.view?.view !== "json") throw new Error("Games plugin returned an unexpected view.");
  model = output.view.data;
  model.mode === "library" ? renderLibrary() : renderGame();
}

async function start() {
  try {
    host = await AssetHubPlugin.connectAssetHubDirectoryFrame();
    await load();
  } catch (cause) {
    app.innerHTML = `<section class="error-state"><h1>Unable to open Games</h1><p>${escapeHtml(cause instanceof Error ? cause.message : String(cause))}</p></section>`;
  }
}

start();
