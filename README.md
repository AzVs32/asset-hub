# Asset Hub

Asset Hub is a local-first asset management system for organizing, browsing,
and extending personal or team asset collections. It provides an HTTP API, a
Web interface, an administration CLI, and a plugin system for supporting
additional resource formats and actions.

The project currently stores metadata in SQLite and asset files on the local
filesystem, making it easy to run directly on a workstation.

## Features

- Organize assets in directories with metadata and content references.
- Address directories by stable UUID and protect Resource and Directory mutations with revisions.
- Manage users and workspace access.
- Upload large files with resumable transfers and checksum verification.
- Extend resource detection, actions, and views with Wasm and Web plugins.
- Display kind-neutral resource and directory thumbnails with nearest-kind Host or plugin
  providers for specialized kinds such as images and EPUB files.
- Run the API, administration CLI, and Web development server directly from source.

## Requirements

- Rust 1.97.1, pinned by `rust-toolchain.toml`.
- Node.js 22.22.2, pinned by `.node-version` and `.nvmrc`.

## Quick Start

### 1. Create the First Administrator

Create the first administrator through the trusted local administration CLI. The password is read
twice from the terminal without being added to the command line:

```bash
cargo run -p asset-cli --bin asset -- user --create admin --admin
```

Administrators use the root workspace. Omitting `--admin` creates a member whose default workspace
is `users/<username>`. If you select a non-default configuration with `--config`, pass the same
path to both `asset` and `asset-http`.

### 2. Start the API

```bash
cargo run -p asset-http --bin asset-http
```

The API listens on `http://127.0.0.1:8080` by default. It can start with no users, but nobody can
log in until an administrator is created with the CLI.

To start with the example configuration:

```bash
cargo run -p asset-http --bin asset-http -- --config config.example.toml
```

Run the following command to see all API options:

```bash
cargo run -p asset-http --bin asset-http -- --help
```

### 3. Start the Web UI

In another terminal:

```bash
cd asset-web
npm install
npm run dev
```

Open `http://127.0.0.1:5173`. The development server proxies `/api` requests
to the API at `http://127.0.0.1:8080`.

### 4. Use the Administration CLI

```bash
cargo run -p asset-cli --bin asset -- --help
```

The CLI provides configuration inspection, user management, system
maintenance, and plugin commands.
See [`asset-cli/README.md`](asset-cli/README.md) for usage details.

## Plugins

Asset Hub supports packaged Wasm runtime plugins with optional Web interfaces.
The repository includes Markdown and EPUB plugins as examples.

See [`asset-plugin-api/README.md`](asset-plugin-api/README.md) for the plugin
authoring contract, package format, compatibility policy, and verification
workflow.

At startup, `asset-runtime` assembles the verified plugin catalog, kind/action
registries, Extism executors, and Core services in that order. `asset-infra`
provides the filesystem verification, Extism, SQLite, OpenDAL, and registry
adapters but does not compose application services. Plugin lock generation is
an explicit packaging step; runtime loading is read-only and requires a valid
`manifest.lock.json`.

Runtime assembly creates one shared directory service for resource, user, and authorization
workflows. It also owns the supervised upload-finalization tasks; Core validates upload state and
executes finalization business logic without spawning detached work.
Resource and Directory kinds/actions use parallel typed contracts: canonical kind/action IDs,
typed built-in or plugin origins, flattened discovered actions, declared output views, and strict
aggregate-identity validation for action effects.

See [`asset-infra/README.md`](asset-infra/README.md) and
[`asset-runtime/README.md`](asset-runtime/README.md) for the adapter/composition
boundary and startup order.

## Development Checks

Run the Rust workspace tests:

```bash
cargo test --workspace
```

Check the Web application:

```bash
cd asset-web
npm run check
npm test
npm run build
```

The bundled plugins have separate build targets and checks. Each build writes only to that
plugin's own `asset-plugin-target/` directory, leaves the root `manifest.json` as the editable
source, and copies a delivery snapshot of it into the target:

```bash
plugins/azvs-markdown/build.sh
plugins/azvs-epub/build.sh
plugins/azvs-games/build.sh
cargo test --manifest-path plugins/azvs-markdown/runtime/Cargo.toml
cargo test --manifest-path plugins/azvs-epub/runtime/Cargo.toml
cargo test --manifest-path plugins/azvs-games/runtime/Cargo.toml
(cd plugins/azvs-markdown/web && npm run typecheck && npm run build)
(cd plugins/azvs-epub/web && npm run typecheck && npm run build)
```
