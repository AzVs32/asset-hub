# AzVs Markdown Plugin

Extism plugin for Asset Hub Markdown document processing.

## Files

- `azvs-markdown.json`: plugin manifest.
- `plugin/Cargo.toml`: Rust plugin crate configuration.
- `plugin/src/`: Rust Extism/WASM action source for the Asset Hub plugin API.
- `web/package.json`: browser viewer package configuration.
- `web/src/`: browser viewer source loaded inside the host iframe.
- `azvs-markdown.wasm`: compiled plugin output, produced by Cargo and copied to the plugin root.
- `web/dist/`: compiled/static browser assets served by Asset Hub.

## Contract

- Plugin ID: `azvs.markdown`
- Parent kind: `core:document`
- Action: `azvs.markdown.render`
- Output view: `plugin_frame`

The action returns an Asset Hub `PluginView` frame payload:

```json
{
  "view": "plugin_frame",
  "title": "note.md",
  "url": "/plugins/azvs.markdown/index.html#payload=..."
}
```

The Rust plugin reads Markdown content, returns a `plugin_frame` view, and
handles writeback through a `replace_content` effect. The frame URL loads
`web/dist/index.html`. The browser viewer uses `markdown-it` to render Markdown,
shows a title tree on the left, and defaults to displaying the full document.

## Build

Build the browser page:

```bash
cd web
npm install
npm run build:web
```

Build the Rust plugin and refresh the root wasm file:

```bash
cd ../plugin
cargo build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/azvs_markdown_plugin.wasm ../azvs-markdown.wasm
```

Rust builds are intentionally explicit, matching the MP4 and EPUB plugins. The
root directory only keeps the manifest, README, and final wasm consumed by Asset
Hub.

Then add `plugins/azvs-markdown/azvs-markdown.json` to
`kind.plugin_manifests` in `config.toml`.
