# AzVs MP4 Plugin

Extism plugin for Asset Hub MP4 playback.

## Files

- `azvs-mp4.json`: Asset Hub plugin manifest.
- `azvs-mp4.wasm`: compiled Extism plugin.
- `src/lib.rs`: Rust source for rebuilding the wasm.

## Resource Kind Extension

- Extends kind: `core:video`
- Matches: `video/mp4`, `.mp4`, `.m4v`
- Action: `azvs.mp4.play`
- Handler: `play_mp4`

## Use

Asset Hub loads plugin manifests listed in `kind.plugin_manifests`. To use this plugin:

1. Add `plugins/azvs-mp4/azvs-mp4.json` to `kind.plugin_manifests` in `config.toml`.
2. Restart Asset Hub.

Upload an MP4 as kind `core:video`. The resource detail panel will show a `Play MP4` action when the MIME type or extension matches MP4.

The plugin returns `view: "html"` with a sandboxed video player UI.

## Build

```bash
cargo build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/azvs_mp4_plugin.wasm azvs-mp4.wasm
```
