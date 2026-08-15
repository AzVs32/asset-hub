# AzVs Markdown Plugin

Extism + React plugin for Asset Hub Markdown document reading and editing.

## Files

- `manifest.json`: editable source of the Asset Hub plugin manifest.
- `runtime`: Rust source for rebuilding the Wasm runtime.
- `web`: React reader/editor source loaded inside the host iframe.
- `asset-plugin-target`: ignored, self-contained installation input containing a generated Manifest
  snapshot, `plugin.wasm`, and the deployable Web bundle.

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
Resource's resolved, write `text_edit` action, whose Manifest explicitly requests
`resource.content.replace`, then forwards the text to the same
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

Install the Web dependencies once:

```bash
cd asset-plugin-api/web
npm ci
cd ../../plugins/azvs-markdown/web
npm ci
cd ../../..
```

Then build this plugin from the repository root:

```bash
plugins/azvs-markdown/build.sh
```

The command builds the local Web SDK dependency, compiles the Wasm runtime, and leaves this
plugin's generated files under:

```text
plugins/azvs-markdown/asset-plugin-target/
├── manifest.json
├── plugin.wasm
├── index.html
└── assets/
```

`plugins/azvs-markdown/manifest.json` remains the only Manifest that authors edit. `build.sh`
refreshes the target copy automatically; it does not generate `manifest.lock.json` or call the
Asset CLI. Lock generation, installation, and verification are intentionally deferred to the
future plugin install workflow.

Individual checks remain available when changing only one side:

```bash
cargo test --manifest-path plugins/azvs-markdown/runtime/Cargo.toml
cd plugins/azvs-markdown/web
npm run typecheck
npm run build
cd ../../..
```
