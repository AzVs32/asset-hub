# AzVs Markdown Plugin

Extism + React plugin for Asset Hub Markdown document reading and editing.

## Files

- `manifest.json`: Asset Hub plugin manifest.
- `plugin.wasm`: compiled Extism plugin artifact used when assembling the installed package.
- `runtime`: Rust source for rebuilding the Wasm runtime.
- `web`: React reader/editor source loaded inside the host iframe.
- `dist`: deployable Web bundle generated from `web`.

## Contract

- Plugin ID: `azvs.markdown`
- Parent kind: `core:text`
- Read action: `azvs.markdown.read` (`render_markdown`)
- Edit action: `azvs.markdown.edit` (`update_markdown`)
- Text capabilities: `text_read` and `text_edit`, replacing the `core:text` fallback providers
- Output view: `plugin_frame`

The initial action returns an Asset Hub `PluginView` frame with a small routing payload:

```json
{
  "view": "plugin_frame",
  "title": "note.md",
  "url": "index.html#payload=<resource-id-mode-action>"
}
```

The URL payload contains only `plugin_api`, `resource_id`, `mode`, and `action`; document content is
never copied into the iframe URL. After loading, the frame connects through
`@asset-hub/plugin-web-sdk` and requests content through its validated Action bridge:

- `{"operation":"load"}` returns UTF-8 Markdown directly up to 512 KiB.
- Larger documents return transfer details and are fetched with sequential
  `{"operation":"chunk","offset":N}` requests using bounded 2 MiB Base64 byte chunks.
- The browser validates the Plugin API, total length, offsets, chunk sizes, completion state,
  Base64, and final UTF-8 before rendering.

The runtime rejects read operations for documents larger than 128 MiB. The effective read maximum
can be lower when the Host's plugin execution policy is lower.

Saving uses the Web SDK's `replaceResourceText`. The Host accepts it only from the frame produced by the current
Resource's resolved, write `text_edit` action, then forwards the text to the same
revision-guarded streaming content use case as the core editor. The Markdown runtime deliberately
rejects the former `{ "markdown": "..." }` Action input and no longer returns a
`replace_content` effect. Consequently, Markdown saves are independent of the 1 MiB Action JSON
limit and are bounded by `resource_edit.max_text_bytes`; an over-limit Resource does not expose the
edit action. The React UI uses `markdown-it` to render Markdown, shows a title tree in read mode,
and provides source editing with live preview in edit mode.

## Build

Prerequisites:

- Rust with the `wasm32-unknown-unknown` target.
- Node.js 22 (`web/.node-version` pins the tested Node.js release).

Install the Rust WebAssembly target once:

```bash
rustup target add wasm32-unknown-unknown
```

Then run the following commands from the repository root:

```bash
cargo build --locked --release --target wasm32-unknown-unknown \
  --manifest-path plugins/azvs-markdown/runtime/Cargo.toml
cp plugins/azvs-markdown/runtime/target/wasm32-unknown-unknown/release/azvs_markdown_plugin.wasm \
  plugins/azvs-markdown/plugin.wasm

cd plugins/azvs-markdown/web
npm ci
npm run typecheck
npm run build
cd ../../..
```

Assemble a clean canonical package and seal it explicitly:

```bash
mkdir -p data/.asset-hub/plugins/azvs.markdown
cp plugins/azvs-markdown/manifest.json plugins/azvs-markdown/plugin.wasm \
  data/.asset-hub/plugins/azvs.markdown/
cp -R plugins/azvs-markdown/dist/. data/.asset-hub/plugins/azvs.markdown/
asset plugin --generate-lock data/.asset-hub/plugins/azvs.markdown/manifest.json
asset plugin --verify data/.asset-hub/plugins/azvs.markdown/manifest.json
```

Asset Hub startup requires and verifies `manifest.lock.json` without modifying it. Asset Hub
discovers the installed package automatically. Its directory name must equal
`plugin.id`; no config entry is required. The source directory keeps rebuild inputs and release
artifacts, while only the canonical package under `.asset-hub/plugins` is loaded. When replacing
the package, remove the old lock, generate a new one, and verify it before restarting.
