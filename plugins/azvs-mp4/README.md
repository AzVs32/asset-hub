# AzVs MP4 Plugin

Extism plugin for Asset Hub MP4 playback.

## Files

- `azvs-mp4.json`: Asset Hub plugin manifest.
- `azvs-mp4.wasm`: compiled Extism plugin.
- `src/lib.rs`: Rust source for rebuilding the wasm.

## Resource Kind

- Kind: `azvs:mp4`
- Action: `azvs:play_mp4`
- Handler: `play_mp4`

## Use

Asset Hub loads plugin manifests from `data/plugins` by default. To use this plugin, either:

1. Copy `azvs-mp4.json` and `azvs-mp4.wasm` into `data/plugins`, then restart Asset Hub.
2. Or add `plugins/azvs-mp4` to `kind.plugin_manifest_dirs` in `config.toml`, then restart Asset Hub.

Upload an MP4 as kind `azvs:mp4`. The resource detail panel will show a `Play MP4` action.

The plugin returns `view: "html"` with a sandboxed video player UI.

## Build

```bash
cargo build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/azvs_mp4_plugin.wasm azvs-mp4.wasm
```
