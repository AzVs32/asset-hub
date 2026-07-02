# Asset Hub

Asset Hub is a local-first asset management service. The current workspace contains:

- `asset-core`: resource domain model, repository/storage/action ports, and resource service.
- `asset-infra`: SQLite repository, OpenDAL Fs blob storage, built-in kinds, plugin manifest loading, and Extism action execution.
- `asset-apps`: reusable runtime assembly plus the `asset-http` Axum API.
- `asset-plugin-api`: shared manifest, action, request, and view contracts for plugins.
- `asset-web-admin`: Vite/React admin UI.
- `plugins`: sample Markdown, EPUB, and MP4 plugins.

## Requirements

- Rust toolchain with the edition 2024 crates supported.
- Node.js for the admin UI and TypeScript plugin checks. On this machine the expected Node path is:

```bash
export PATH=/storage/apps/node-v22.20.0/bin:$PATH
```

## Run The API

Start the HTTP API from the repository root:

```bash
cargo run -p asset-apps --bin asset-http
```

Defaults:

- API listens on `127.0.0.1:8080`.
- SQLite database is `data/asset-hub.sqlite`.
- Blob storage root is `data/blob`.
- If `config.toml` exists, it is loaded. Otherwise built-in defaults are used.

Use a config file:

```bash
ASSET_HUB_CONFIG=config.example.toml cargo run -p asset-apps --bin asset-http
```

## HTTP Boundary Settings

These environment variables control the HTTP shell around the API:

- `ASSET_HTTP_ADDR`: listen address, default `127.0.0.1:8080`.
- `ASSET_HTTP_ENABLE_SWAGGER`: expose `/swagger-ui` and `/api-docs/openapi.json`, default `true`.
- `ASSET_HTTP_ENABLE_PURGE`: expose physical delete endpoint `DELETE /resources/{id}/purge`, default `true` for local development.
- `ASSET_HTTP_CORS_ALLOWED_ORIGINS`: comma-separated origins, or `*`. Empty/unset disables CORS headers.
- `ASSET_HTTP_REQUEST_TIMEOUT_SECS`: request timeout, default `30`.

Example production-leaning local run:

```bash
ASSET_HTTP_ENABLE_SWAGGER=false \
ASSET_HTTP_ENABLE_PURGE=false \
ASSET_HTTP_CORS_ALLOWED_ORIGINS=http://127.0.0.1:5173 \
cargo run -p asset-apps --bin asset-http
```

## Run The Admin UI

In one terminal, start the API. In another:

```bash
cd asset-web-admin
npm install
npm run dev
```

Vite serves `http://127.0.0.1:5173` and proxies `/api` to `http://127.0.0.1:8080`.

To point the UI at a different API origin:

```bash
VITE_API_BASE_URL=http://127.0.0.1:8080 npm run dev
```

## Plugins

Plugin manifest paths are configured under `[kind].plugin_manifests` in `config.toml`.
Manifest paths are loaded at API startup; restart the service after changing plugin manifests or WASM files.

Example:

```toml
[kind]
plugin_manifests = [
  "plugins/azvs-markdown/azvs-markdown.json",
  "plugins/azvs-epub/azvs-epub.json",
  "plugins/azvs-mp4/azvs-mp4.json",
]
```

## Checks

Run the local check script:

```bash
./scripts/check-local.sh
```

The script runs:

- `cargo test --workspace`
- `cargo test --manifest-path plugins/azvs-epub/Cargo.toml`
- `cargo test --manifest-path plugins/azvs-mp4/Cargo.toml`
- `npm run lint` and `npm run build` in `asset-web-admin`
- `npm test` in `plugins/azvs-markdown`

Individual checks:

```bash
cargo test --workspace
cd asset-web-admin && npm run lint && npm run build
cd plugins/azvs-markdown && npm test
```
