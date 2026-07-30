# Asset Hub

Asset Hub is a local-first asset management system for organizing, browsing,
and extending personal or team asset collections. It provides an HTTP API, a
Web interface, an administration CLI, and a plugin system for supporting
additional resource formats and actions.

The project currently stores metadata in SQLite and asset files on the local
filesystem, making it easy to run on a workstation or self-host with Docker.

## Features

- Organize assets in directories with metadata and tags.
- Manage users and workspace access.
- Upload large files with resumable transfers and checksum verification.
- Extend resource detection, actions, and views with Wasm and Web plugins.
- Run locally from source or deploy with Docker Compose.

## Requirements

- Rust 1.97.1, pinned by `rust-toolchain.toml`.
- Node.js 22.22.2, pinned by `.node-version` and `.nvmrc`.

Docker is sufficient when using the Compose deployment.

## Quick Start

### 1. Start the API

On the first startup, provide an administrator username and password:

```bash
ASSET_HUB_BOOTSTRAP_ADMIN_USERNAME=admin \
ASSET_HUB_BOOTSTRAP_ADMIN_PASSWORD='replace-with-a-long-password' \
cargo run -p asset-http --bin asset-http
```

The API listens on `http://127.0.0.1:8080` by default. Bootstrap credentials
are used only when no users exist.

To start with the example configuration:

```bash
cargo run -p asset-http --bin asset-http -- --config config.example.toml
```

Run the following command to see all API options:

```bash
cargo run -p asset-http --bin asset-http -- --help
```

### 2. Start the Web UI

In another terminal:

```bash
cd asset-web
npm install
npm run dev
```

Open `http://127.0.0.1:5173`. The development server proxies `/api` requests
to the API at `http://127.0.0.1:8080`.

### 3. Use the Administration CLI

```bash
cargo run -p asset-cli --bin asset -- --help
```

The CLI provides configuration inspection, user management, system
maintenance, and plugin commands.
See [`asset-cli/README.md`](asset-cli/README.md) for usage details.

## Docker

Create the deployment environment file and set a bootstrap administrator
password before starting the stack:

```bash
cp docker/.env.example docker/.env
# Edit docker/.env before continuing.
cd docker
docker compose up -d --build
```

Open `http://127.0.0.1:8080` after the containers start.

See [`docker/README.md`](docker/README.md) for usage details.

## Plugins

Asset Hub supports packaged Wasm runtime plugins with optional Web interfaces.
The repository includes Markdown and EPUB plugins as examples.

See [`asset-plugin-api/README.md`](asset-plugin-api/README.md) for the plugin
authoring contract, package format, compatibility policy, and verification
workflow.

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

The bundled plugins have their own Rust and Web checks:

```bash
cargo test --manifest-path plugins/azvs-markdown/runtime/Cargo.toml
cargo test --manifest-path plugins/azvs-epub/runtime/Cargo.toml
(cd plugins/azvs-markdown/web && npm run typecheck && npm run build)
(cd plugins/azvs-epub/web && npm run typecheck && npm run build)
```
