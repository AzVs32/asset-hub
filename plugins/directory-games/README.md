# Directory Games plugin

`directory.games` contributes a dedicated React workspace for `directory:games` and game entries
of kind `directory:games:item`.

The Host supplies bounded child/resource reads and the generic `create_tree` effect. This plugin
owns the game model: creating a game uses its required printable-ASCII English name as the Directory
name and emits `README.md` and `METADATA.yml`. An optional PNG, JPEG, WebP, GIF, or SVG icon (up to
1 MiB) is stored as `public/cover.<ext>` inside the game Directory. Raster formats are identified
from their content, decoded with bounded dimensions and memory, and stored unchanged under the
matching canonical extension even when the submitted MIME type names another supported image
format; the submitted MIME type is advisory and is not trusted for validation. SVG icons are parsed
as bounded static images with external file references disabled, then serialized from the normalized
SVG tree before storage; scripts, animation, event handlers, and other unsupported dynamic content
are not preserved. SVG text should be converted to paths because the runtime deliberately does not
load fonts. The game workspace displays the cover in its left rail and uses the built-in game icon
when no cover is available. Library cards load the same cover lazily and otherwise use that fallback
icon.
Optional Unicode aliases are stored with the English name in a YAML array:

```yaml
name:
  - "English Name"
  - "别名"
```

The Rust runtime consumes these capabilities through bounded `DirectoryContext` queries and builds
the scaffold with the SDK `Tree` response builder. The React frame imports
`@asset-hub/plugin-web-sdk`, loads workspace data through `directory.games.workspace`, invokes
`directory.games.create`, and delegates Directory navigation and document viewing/editing to the
Host. The game workspace shows `README.md` through the resolved `view` provider and mounts its
exact `plugin_frame`. The Web SDK relays that nested frame's standard Resource calls through the
Directory-bound Host connection. Editing uses the symmetric resolved `edit` provider.
Directory Games does not read document content in its runtime, select either provider Action ID,
render Markdown, implement text saving, or request Resource write permission.

Generated Markdown resources do not name a Kind owned by another plugin. Directory Games leaves
their Kind unset in the `create_tree` output, allowing the Host to detect an installed
format-specific Kind or fall back to `core:resource`. Reading and editing are available when the
Resource exposes matching `view` and `edit` providers.

`directory:games:item` inherits from `directory:games`, so the Kind hierarchy reflects that a game
entry belongs to the Games model and receives the same workspace capabilities. This inheritance
does not restrict physical placement: users may assign `directory:games:item` to a game directory
under any Directory Kind. The Games Kind declares it as `default_child_kind`, so new generic direct
children and existing generic children present when a Directory becomes Games are automatically
reclassified as game entries. Explicit non-Core child Kinds are preserved.

## Build

Requires Rust with the `wasm32-unknown-unknown` target and Node.js 22
(`web/.node-version` pins the tested Node.js release). Prepare them once:

```bash
rustup target add wasm32-unknown-unknown
cd asset-plugin-sdk/web
npm ci
cd ../../plugins/directory-games/web
npm ci
cd ../../..
```

Build and install from the repository root:

```bash
plugins/directory-games/build.sh
cargo run --bin asset plugin --install plugins/directory-games/asset-plugin-target
```

<details>
<summary>Build details</summary>

`build.sh` rebuilds the local Web SDK dependency, compiles the Wasm runtime, builds the React
application, and writes these generated files to:

```text
plugins/directory-games/asset-plugin-target/
├── manifest.json
├── plugin.wasm
├── index.html
└── assets/
```

The root `manifest.json` remains the only Manifest that authors edit. The build refreshes its target
snapshot and does not generate `manifest.lock.json` or call the Asset CLI. `asset plugin --install`
snapshots the target, generates and verifies the installed lock, and does not modify
`asset-plugin-target`.

Run the runtime and Web checks independently with:

```bash
cargo test --manifest-path plugins/directory-games/runtime/Cargo.toml
cd plugins/directory-games/web
npm run typecheck
npm run lint
npm run build
cd ../../..
```

</details>
