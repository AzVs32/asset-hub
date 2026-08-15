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

## Build

Requires Rust with the `wasm32-unknown-unknown` target and Node.js 22. Prepare them once:

```bash
rustup target add wasm32-unknown-unknown
cd asset-plugin-api/web
npm ci
cd ../..
```

Build and install from the repository root:

```bash
plugins/azvs-games/build.sh
cargo run --bin asset plugin --install plugins/azvs-games/asset-plugin-target
```

<details>
<summary>Build details</summary>

Games uses plain HTML, CSS, and JavaScript rather than React/Vite. `build.sh` compiles the Wasm
runtime, rebuilds the global Asset Hub Web SDK bundle, and assembles these files:

```text
plugins/azvs-games/asset-plugin-target/
├── manifest.json
├── plugin.wasm
├── index.html
├── app.js
├── styles.css
└── asset-hub-plugin.global.js
```

The root `manifest.json` remains the only Manifest that authors edit. The build refreshes its target
snapshot and keeps this plugin's output separate from the other plugin targets. `build.sh` does not
generate `manifest.lock.json` or call the Asset CLI. `asset plugin --install` snapshots the target,
generates and verifies the installed lock, and does not modify `asset-plugin-target`.

Run the runtime checks independently with:

```bash
cargo test --manifest-path plugins/azvs-games/runtime/Cargo.toml
```

</details>
