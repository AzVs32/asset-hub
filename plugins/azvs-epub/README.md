# AzVs EPUB Plugin

Extism plugin for Asset Hub EPUB reading.

## Files

- `azvs-epub.json`: Asset Hub plugin manifest.
- `azvs-epub.wasm`: compiled Extism plugin.
- `src/lib.rs`: Rust source for rebuilding the wasm.

## Resource Kind

- Kind: `azvs:epub`
- Action: `azvs:render_epub`
- Handler: `render_epub`

## Use

Asset Hub loads plugin manifests listed in `kind.plugin_manifests`. To use this plugin:

1. Add `plugins/azvs-epub/azvs-epub.json` to `kind.plugin_manifests` in `config.toml`.
2. Restart Asset Hub.

Upload an EPUB as kind `azvs:epub`. The resource detail panel will show a `Read EPUB` action.

The plugin returns `view: "html"` with a sandboxed reader UI containing cover preview and chapter navigation.

## Build

```bash
cargo build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/azvs_epub_plugin.wasm azvs-epub.wasm
```
