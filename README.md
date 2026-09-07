# Asset Hub

Asset Hub is a local-first asset management system. It serves one global asset workspace through a
Web interface and local administration commands, and is built with a hexagonal architecture around
two aggregates:

- `Resource`: an asset with its metadata and content reference.
- `Directory`: an independent hierarchy node identified by a stable UUID.

## Prerequisites

- Rust (the pinned toolchain is in [rust-toolchain.toml](rust-toolchain.toml); `rustup` installs it
  automatically).
- Node.js 22.12 or later for the Web interface.

## Quick start

Optionally copy [config.example.toml](config.example.toml) to `config.toml` and adjust it. Without a
configuration file the built-in defaults apply: blob storage rooted at `data/` and the SQLite
database at `data/.asset-hub/asset-hub.sqlite`.

Install the Web application's dependencies:

```bash
npm --prefix asset-web ci
```

Start the HTTP service:

```bash
cargo run -p asset-http --bin asset-http
```

It listens on `http://127.0.0.1:8080` by default. `--addr` changes the listen address, `--config`
selects a configuration file, and `asset-http --help` shows the full contract. In another terminal,
start the Web interface:

```bash
cd asset-web
npm run dev
```

Open `http://127.0.0.1:5173`. The browser opens directly to the global asset workspace: browse
directories, upload resources, edit text content, and download or delete assets. Startup creates
the required local data automatically.

## Managing assets locally

Resource and Directory operations always run through the Core application services. The `asset` CLI
adds local administration on top:

```bash
cargo run -p asset-cli --bin asset -- config --check
cargo run -p asset-cli --bin asset -- system --scan-resource
```

`asset config` validates and shows the effective configuration; `asset system --scan-resource`
re-scans blob storage, recomputes every SHA-256, and reconciles the resource database. See
[asset-cli/README.md](asset-cli/README.md) for the full command reference.

## Configuration

The executables read `--config <PATH>` when given, then `./config.toml`, then built-in defaults.
[config.example.toml](config.example.toml) documents every key:

- `[database]` selects the backend (SQLite) and its connection pool size.
- `[resource_edit]` limits interactive text edits; `[idempotency]` controls request leases.
- `[blob]` selects the backend (local filesystem) and its root directory; `[blob.local.sync]` keeps
  the database reconciled with changes made directly in the file system.

## Repository map

- `asset-core`: domain, ports, and application services.
- `asset-infra`: SQLite repositories, OpenDAL storage, and filesystem adapters.
- `asset-runtime`: reusable runtime assembly and background-task ownership.
- `asset-http`: Axum transport, DTOs, OpenAPI, and the HTTP executable.
- `asset-cli`: administration commands and the CLI executable.
- `asset-web`: the React browser application.

Each module directory documents its own architecture, contract, and test commands.

## Development

```bash
cargo fmt --all
cargo clippy --workspace --all-targets
cargo test --workspace
npm --prefix asset-web run check
```

The generated OpenAPI document is served at
`http://127.0.0.1:8080/api-docs/openapi.json`; `asset-web` regenerates its transport types from it
with `npm run generate:api` (see [asset-http/README.md](asset-http/README.md)).
