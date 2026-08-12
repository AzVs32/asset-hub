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

- `manifest.json`: Asset Hub plugin manifest.
- `plugin.wasm`: compiled Extism plugin artifact used when assembling the installed package.
- `runtime`: Rust source for rebuilding the Wasm runtime.
- `web`: React reader source.
- `dist`: deployable Web bundle generated from `web`.

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
  --manifest-path plugins/azvs-epub/runtime/Cargo.toml
cp plugins/azvs-epub/runtime/target/wasm32-unknown-unknown/release/azvs_epub_plugin.wasm \
  plugins/azvs-epub/plugin.wasm

cd plugins/azvs-epub/web
npm ci
npm run typecheck
npm run build
cd ../../..
```

Then assemble a clean package and seal it explicitly:

```bash
mkdir -p data/.asset-hub/plugins/azvs.epub
cp plugins/azvs-epub/manifest.json plugins/azvs-epub/plugin.wasm \
  data/.asset-hub/plugins/azvs.epub/
cp -R plugins/azvs-epub/dist/. data/.asset-hub/plugins/azvs.epub/
asset plugin --seal azvs.epub
asset plugin --verify azvs.epub
```

Asset Hub startup requires and verifies `manifest.lock.json` without modifying it. When replacing
the package artifacts, remove the old lock, generate a new one, and verify it before restarting.
