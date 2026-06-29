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

Asset Hub loads plugin manifests from `data/plugins` by default. To use this plugin, either:

1. Copy `azvs-epub.json` and `azvs-epub.wasm` into `data/plugins`, then restart Asset Hub.
2. Or add `plugins/azvs-epub` to `kind.plugin_manifest_dirs` in `config.toml`, then restart Asset Hub.

Upload an EPUB as kind `azvs:epub`. The resource detail panel will show a `Read EPUB` action.

The plugin returns `view: "html"` with a sandboxed reader UI containing cover preview and chapter navigation.

## Build

```bash
cargo build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/azvs_epub_plugin.wasm azvs-epub.wasm
```
