# AzVs EPUB Plugin

Extism + React plugin for reading EPUB 2 and EPUB 3 books in Asset Hub.

## Reader capabilities

- EPUB container, OPF spine, EPUB 2 NCX, and EPUB 3 navigation documents.
- EPUB 2/3 cover discovery, including guide cover pages.
- Lazy chapter loading with an in-process, version-keyed LRU cache.
- Chapter images, SVG images, audio, video, CSS, and embedded fonts rewritten to data URLs.
- Same-chapter anchors, cross-chapter links, and footnote navigation.
- Book CSS isolated in a Shadow DOM so it cannot restyle the reader interface.
- HTML parsed and rewritten by `lol_html`, then sanitized with the Ammonia allowlist sanitizer.
- Bounded archive, entry, chapter, and expanded-resource sizes to limit malformed ZIP impact.

The plugin intentionally does not execute book scripts or load remote resources. It does not support
DRM, EPUB media-overlay timing, or full fixed-layout pagination. Unsupported resources degrade to
their fallback text or are omitted. The cache is bounded and process-local; it avoids repeated work
during normal browsing but is cleared when Asset Hub restarts.

## Files

- `manifest.json`: editable source of the Asset Hub plugin manifest.
- `runtime`: Rust source for rebuilding the Wasm runtime.
- `web`: React reader source.
- `asset-plugin-target`: ignored, self-contained installation input containing a generated Manifest
  snapshot, `plugin.wasm`, and the deployable Web bundle.

## Resource Kind

- Kind: `azvs:epub`
- Parent kind: `core:resource`
- Action: `azvs.epub.render`
- Handler: `render_epub`
- Thumbnail action: `azvs.epub.thumbnail`, providing the Resource-scoped singleton `thumbnail`
  capability in `resource_thumbnail`

## Use

Asset Hub automatically loads a canonical package installed at
`<blob.local.root>/.asset-hub/plugins/azvs.epub`. Restart Asset Hub after installing or changing it.

Upload an EPUB as kind `azvs:epub`. The resource row context menu will show a `Read` action.

The first action call returns a `plugin_frame`. The React reader connects through
`@asset-hub/plugin-web-sdk`, then requests a book index and the first chapter through the Asset Hub
frame bridge. Additional chapters are fetched on demand and cached by the reader and Wasm runtime.
Book content has no direct network access.

The plugin runtime accepts two private bridge operations on `azvs.epub.render`:

- `{"operation":"load"}` returns book information, chapter summaries, cover, and the first chapter.
- `{"operation":"chapter","index":N}` returns one sanitized chapter and its isolated styles.

## Build

Requires Rust with the `wasm32-unknown-unknown` target and Node.js 22
(`web/.node-version` pins the tested Node.js release). Prepare them once:

```bash
rustup target add wasm32-unknown-unknown
cd asset-plugin-api/web
npm ci
cd ../../plugins/azvs-epub/web
npm ci
cd ../../..
```

Build and install from the repository root:

```bash
plugins/azvs-epub/build.sh
cargo run --bin asset plugin --install plugins/azvs-epub/asset-plugin-target
```

<details>
<summary>Build details</summary>

`build.sh` rebuilds the local Web SDK dependency, compiles the Wasm runtime, builds the React
application, and writes this plugin's generated files to:

```text
plugins/azvs-epub/asset-plugin-target/
├── manifest.json
├── plugin.wasm
├── index.html
└── assets/
```

`plugins/azvs-epub/manifest.json` remains the only Manifest that authors edit. `build.sh` refreshes
the target copy automatically; it does not generate `manifest.lock.json` or call the Asset CLI.
`asset plugin --install` snapshots the target, generates and verifies the installed lock, and does
not modify `asset-plugin-target`.

Individual checks remain available when changing only one side:

```bash
cargo test --manifest-path plugins/azvs-epub/runtime/Cargo.toml
cd plugins/azvs-epub/web
npm run typecheck
npm run build
cd ../../..
```

</details>
