# Games plugin

`azvs.games` contributes a dedicated workspace for `directory:games` and game entries of kind
`directory:games:item`.

The Host only supplies bounded child/resource reads and the generic `create_tree` effect. This
plugin owns the game model: creating a game emits a game directory, `public/`, a generated
`README.md`, and an empty `HASH.md` reserved for a later integrity workflow. The README is the
default content rendered by both the library card and game workspace.

`directory:games:item` inherits from `directory:games`, so the Kind hierarchy reflects that a game
entry belongs to the Games model and receives the same workspace capabilities. This inheritance
does not restrict physical placement: users may assign `directory:games:item` to a game directory
under any Directory Kind, without first creating a Games library. The Games Kind still declares it
as `default_child_kind`, so new generic direct children and existing generic children present when
a Directory becomes Games are automatically reclassified as game entries. Explicit non-Core child
Kinds are preserved.

## Build output

The root `manifest.json` remains the only Manifest that authors edit. The build copies a delivery
snapshot of it beside the generated runtime and Web artifacts under this plugin's ignored
`asset-plugin-target/` directory; files are not mixed with either of the other plugin targets.

Install the Asset Hub Web SDK dependencies once, then run the Games build from the repository root:

```bash
cd asset-plugin-api/web
npm ci
cd ../..
plugins/azvs-games/build.sh
```

The result is:

```text
plugins/azvs-games/asset-plugin-target/
├── manifest.json
├── plugin.wasm
├── index.html
├── app.js
├── styles.css
└── asset-hub-plugin.global.js
```

`build.sh` does not generate `manifest.lock.json` or call the Asset CLI. Lock generation,
installation, and verification are intentionally deferred to the future plugin install workflow.
