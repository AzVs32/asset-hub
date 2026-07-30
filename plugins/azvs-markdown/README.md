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
- Parent kind: `core:document`
- Read action: `azvs.markdown.render` (`render_markdown`)
- Edit action: `azvs.markdown.update` (`update_markdown`)
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
never copied into the iframe URL. After loading, the frame requests content through Asset Hub's
validated `postMessage` action bridge:

- `{"operation":"load"}` returns UTF-8 Markdown directly up to 512 KiB.
- Larger documents return transfer details and are fetched with sequential
  `{"operation":"chunk","offset":N}` requests using bounded 2 MiB Base64 byte chunks.
- The browser validates the Plugin API, total length, offsets, chunk sizes, completion state,
  Base64, and final UTF-8 before rendering.

The runtime rejects documents larger than 128 MiB. The effective maximum can be lower when the
host's `plugin.max_content_bytes` policy is lower.

The Rust plugin handles writeback through the existing controlled `replace_content` effect. Saving
does not write storage directly and remains subject to the host's action-input and plugin-output
limits. The React UI uses `markdown-it` to render Markdown, shows a title tree in read mode, and
provides source editing with live preview in edit mode.

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

Assemble a clean, lock-free canonical package:

```bash
mkdir -p data/.asset-hub/plugins/azvs.markdown
cp plugins/azvs-markdown/manifest.json plugins/azvs-markdown/plugin.wasm \
  data/.asset-hub/plugins/azvs.markdown/
cp -R plugins/azvs-markdown/dist/. data/.asset-hub/plugins/azvs.markdown/
```

Asset Hub generates `manifest.lock.json` during the first startup and verifies it on later startups.
Asset Hub discovers the installed package automatically. Its directory name must equal
`plugin.id`; no config entry is required. The source directory keeps rebuild inputs and release
artifacts, while only the canonical package under `.asset-hub/plugins` is loaded. When replacing
the package, do not retain a lock generated for older artifacts.
