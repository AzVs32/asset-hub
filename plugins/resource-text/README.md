# Resource Text Plugin

`resource.text` owns text-file reading and editing outside the Asset Hub Host. It does not register a
generic text kind. Markdown keeps its concrete `resource:markdown` kind, while basic text and source
files remain `core:resource` and receive Actions through MIME or extension matching.

## Files

- `manifest.json`: editable source of the Asset Hub plugin manifest.
- `runtime`: Rust source for rebuilding the Wasm runtime.
- `web`: React reader/editor source loaded inside the host iframe.
- `asset-plugin-target`: ignored, self-contained installation input containing a generated Manifest
  snapshot, `plugin.wasm`, and the deployable Web bundle.

## Contract

- Plugin ID: `resource.text`
- Markdown kind: `resource:markdown`
- Parent kind: `core:resource`
- Read action: `resource.text.read` (`read_text`)
- Edit action: `resource.text.edit` (`edit_text`)
- Read capability: `view`; edit capability: `edit`
- Output view: `plugin_frame`

The Host has no generic text kind or fallback text provider. Markdown MIME types and extensions are
detected by this plugin's concrete `resource:markdown` declaration. Both Actions are declared on
`core:resource`, match Markdown MIME types or supported extensions, and are inherited by
`resource:markdown`.

| Rendering | Extensions | Persisted Kind |
| --- | --- | --- |
| Markdown reader/editor with preview | `.md`, `.markdown`, `.mdown`, `.mkd` | `resource:markdown` |
| Basic plain-text reader/editor | `.txt`, `.c`, `.cpp`, `.h`, `.yaml`, `.yml` | `core:resource` |

YAML matching accepts `application/yaml`, `application/x-yaml`, `text/yaml`, and `text/x-yaml`.

To add another basic text extension later, append it to the `extensions` list of both Actions in
`manifest.json`. It will use the plain-text interface without requiring another Kind.

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

- `{"operation":"load"}` returns UTF-8 text directly up to 512 KiB.
- Larger documents return transfer details and are fetched with sequential
  `{"operation":"chunk","offset":N}` requests using bounded 2 MiB Base64 byte chunks.
- The browser validates the Plugin API, total length, offsets, chunk sizes, completion state,
  Base64, and final UTF-8 before rendering.

The runtime rejects read operations for text larger than 128 MiB. The effective read maximum
can be lower when the Host's plugin execution policy is lower.
Inline bytes and opaque Host content handles are consumed through the same bounded
`ResourceContext::content` SDK interface; the plugin does not manage ABI handles or conditional
Wasm implementations.

Saving uses the Web SDK's `replaceResourceText`. The Host accepts it only from the frame produced by the current
Resource's resolved, write `edit` action, whose Manifest explicitly requests
`resource.content.replace`, then forwards the text to the same
revision-guarded streaming content replacement use case. The runtime rejects inline writeback
through Action JSON and does not return a `replace_content` effect. Consequently, saves are
independent of the 1 MiB Action JSON limit and are bounded by `resource_edit.max_text_bytes`; an
over-limit Resource does not expose the edit action. The React UI uses `markdown-it` for Markdown
headings and preview. Selecting a heading keeps the reader's heading sidebar open until the user
explicitly toggles it from the toolbar. Basic text, source, and YAML files use a monospaced
reader/editor.

## Build

Requires Rust with the `wasm32-unknown-unknown` target and Node.js 22
(`web/.node-version` pins the tested Node.js release). Prepare them once:

```bash
rustup target add wasm32-unknown-unknown
cd asset-plugin-sdk/web
npm ci
cd ../../plugins/resource-text/web
npm ci
cd ../../..
```

Build and install from the repository root:

```bash
plugins/resource-text/build.sh
cargo run --bin asset plugin --install plugins/resource-text/asset-plugin-target
```

<details>
<summary>Build details</summary>

`build.sh` rebuilds the local Web SDK dependency, compiles the Wasm runtime, builds the React
application, and writes this plugin's generated files to:

```text
plugins/resource-text/asset-plugin-target/
├── manifest.json
├── plugin.wasm
├── index.html
└── assets/
```

`plugins/resource-text/manifest.json` remains the only Manifest that authors edit. `build.sh`
refreshes the target copy automatically; it does not generate `manifest.lock.json` or call the
Asset CLI. `asset plugin --install` snapshots the target, generates and verifies the installed lock,
and does not modify `asset-plugin-target`.

Individual checks remain available when changing only one side:

```bash
cargo test --manifest-path plugins/resource-text/runtime/Cargo.toml
cd plugins/resource-text/web
npm run typecheck
npm run build
cd ../../..
```

</details>
