# Hello Directory Plugin

Minimal Directory workspace plugin for Asset Hub. It renders a self-contained static page and is
intended as a reference for a Directory Kind that completely owns its internal workspace UI.

## Contract

- Plugin ID: `azvs.directory.hello`
- Directory Kind: `azvs:directory.hello`
- Parent Kind: `core:directory`
- Thumbnail action: `azvs.directory.hello.thumbnail` (`render_thumbnail`)
- Workspace action: `azvs.directory.hello.workspace` (`render_workspace`)
- Provided capabilities: `thumbnail` and `workspace`
- Output views: `media` and `plugin_frame`
- UI locations: `directory_thumbnail` and `directory_workspace`
- Permission: `directory.read`

The thumbnail handler returns a self-contained blue SVG and replaces the inherited Core Directory
thumbnail for `azvs:directory.hello`. The workspace handler validates the Directory Action request
and returns `index.html` as a plugin frame. The page is deliberately static: it does not connect to
the Directory Frame Bridge, list children, read resources, or request any write permission.

## Build

Install the Rust WebAssembly target once:

```bash
rustup target add wasm32-unknown-unknown
```

Then run these commands from the repository root:

```bash
cargo build --locked --release --target wasm32-unknown-unknown \
  --manifest-path plugins/azvs-directory-hello/runtime/Cargo.toml
cp plugins/azvs-directory-hello/runtime/target/wasm32-unknown-unknown/release/azvs_directory_hello_plugin.wasm \
  plugins/azvs-directory-hello/plugin.wasm
cp plugins/azvs-directory-hello/web/index.html plugins/azvs-directory-hello/dist/index.html
```

## Install

Assemble and seal a canonical package:

```bash
mkdir -p data/.asset-hub/plugins/azvs.directory.hello
cp plugins/azvs-directory-hello/manifest.json plugins/azvs-directory-hello/plugin.wasm \
  data/.asset-hub/plugins/azvs.directory.hello/
cp -R plugins/azvs-directory-hello/dist/. data/.asset-hub/plugins/azvs.directory.hello/
cargo run --bin asset plugin --seal azvs.directory.hello
cargo run --bin asset plugin --verify azvs.directory.hello
```

Restart Asset Hub after installation. Change a Directory's Kind to `Hello Directory`; the Host keeps
the shared navigation outside the workspace and renders this plugin's static page inside it.
