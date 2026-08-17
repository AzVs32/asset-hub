# Resource Image Plugin

`resource.image` provides image thumbnails outside the Asset Hub Host. It intentionally registers
no Resource Kind: matching files remain `core:resource`, and the thumbnail Action is selected by
MIME type or filename extension.

## Files

- `manifest.json`: editable source of the Asset Hub plugin manifest.
- `runtime`: Rust source for rebuilding the Wasm runtime.
- `asset-plugin-target`: ignored, self-contained installation input containing a generated Manifest
  snapshot and `plugin.wasm`.

## Contract

- Plugin ID: `resource.image`
- Kind declarations: none
- Thumbnail action: `resource.image.thumbnail` (`render_thumbnail`)
- Thumbnail capability: `thumbnail`
- Output view: URL-encoded `media` pointing to the authorized Resource content endpoint
- Permission: `resource.read`; the Wasm runtime does not read or copy image bytes

The Action applies to `image/*` or these common extensions: `.png`, `.jpg`, `.jpeg`, `.gif`,
`.webp`, `.svg`, `.bmp`, `.avif`, `.ico`, `.tif`, and `.tiff`. An available image MIME type is
preserved in normalized form; when matching by extension, the runtime supplies the corresponding
image MIME type to the media view.

The runtime imports the high-level authoring API directly from `asset-plugin-sdk`; the SDK export
macro owns Extism/wire serialization and `Media::url` expresses the thumbnail response without
constructing protocol DTOs.

## Build

Requires Rust with the `wasm32-unknown-unknown` target. Prepare it once:

```bash
rustup target add wasm32-unknown-unknown
```

Build and install from the repository root:

```bash
plugins/resource-image/build.sh
cargo run --bin asset plugin --install plugins/resource-image/asset-plugin-target
```

<details>
<summary>Build details</summary>

`build.sh` compiles the Wasm runtime and writes this plugin's generated files to:

```text
plugins/resource-image/asset-plugin-target/
├── manifest.json
└── plugin.wasm
```

`plugins/resource-image/manifest.json` remains the only Manifest that authors edit. `build.sh`
refreshes the target copy automatically; it does not generate `manifest.lock.json` or call the
Asset CLI. `asset plugin --install` snapshots the target, generates and verifies the installed lock,
and does not modify `asset-plugin-target`.

The runtime can be checked independently with:

```bash
cargo test --manifest-path plugins/resource-image/runtime/Cargo.toml
```

</details>
