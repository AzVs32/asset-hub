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

It listens on `http://127.0.0.1:8080` by default. HTTP behavior is configured under `[http]`;
`--config` selects the shared configuration file. In another terminal, start the Web interface:

```bash
cd asset-web
npm run dev
```

Open `http://127.0.0.1:5173`. The browser opens directly to the global asset workspace: browse
directories, upload resources, edit resource names and locations, and download or delete assets. Startup creates
the required local data automatically.

Resumable binary content replacement is available through the HTTP upload API; the Web interface
currently edits resource metadata only. New resources and replacement content use sequential
8 MiB-or-smaller chunks without imposing a total resource-size limit.

## Managing assets locally

Resource and Directory operations always run through the Core application services. The `asset` CLI
adds local administration on top:

```bash
cargo run -p asset-cli --bin asset -- config --check
cargo run -p asset-cli --bin asset -- system --scan-resource
```

`asset config` validates and shows the effective `[asset]` core configuration while retaining
unregistered extension sections; `asset system --scan-resource` re-scans blob storage, recomputes
every SHA-256, and reconciles the resource database. See
[asset-cli/README.md](asset-cli/README.md) for the full command reference.

## Configuration

The executables read `--config <PATH>` when given, then `./config.toml`, then built-in defaults.
[config.example.toml](config.example.toml) documents every built-in key. Each component owns a
strongly typed section; each executable registers the sections it consumes and loads the shared
document once:

- `[asset]` contains the core runtime settings: database, Blob storage, and idempotency.
- `[http]` contains the HTTP listener, CORS, request timeout, and archive limits.

Additional terminals and plugins can register independent sections without adding fields or
dependencies to either built-in configuration type. A registered section strictly validates its
own subtree; unregistered sections remain available for another executable or a later-loaded plugin.

## Repository map

- `asset-core`: capability-oriented domain kernel. `directory`, `resource`, and `idempotency`
  each keep their domain, ports, and application services together; `storage` contains storage
  ports and `workflow` contains cross-aggregate application workflows. See
  [asset-core AI development instructions](asset-core/AGENTS.md).
- `asset-config`: generic shared configuration loading and extensible typed section registration.
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
