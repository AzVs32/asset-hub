# Asset Hub

Asset Hub is a local-first asset management service. The current workspace contains:

- `asset-core`: resource domain model, repository/storage/action ports, and resource service.
- `asset-infra`: SQLite repository, OpenDAL Fs blob storage, built-in kinds, plugin manifest loading, and Extism action execution.
- `asset-apps`: reusable runtime assembly plus the `asset-http` Axum API.
- `asset-plugin-api`: shared manifest, action, request, and view contracts for plugins.
- `asset-web-admin`: Vite/React admin UI.
- `plugins`: sample Markdown, EPUB, and MP4 plugins.

## Requirements

- Rust 1.94.1 (pinned by `rust-toolchain.toml`).
- Node.js 22.22.2 (pinned by `.node-version`; compatible with fnm) and npm 10.

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

## Users And Directory Access

The HTTP API uses SQLite-backed login sessions. On the first startup, provide an
initial administrator; the password is stored as an Argon2 hash and is never
written to configuration:

```bash
ASSET_HUB_BOOTSTRAP_ADMIN_USERNAME=admin \
ASSET_HUB_BOOTSTRAP_ADMIN_PASSWORD='replace-with-a-long-password' \
cargo run -p asset-apps --bin asset-http
```

The bootstrap values are only used when the `users` table is empty. Later
starts do not require them.

Authentication endpoints:

- `POST /auth/login` with `{ "username": "...", "password": "..." }`
- `POST /auth/logout`
- `GET /auth/me`
- `POST /auth/users` (administrator only)
- `PUT /auth/directory-grants` (administrator only), with `user_id`,
  `directory`, and `permission` (`read`, `write`, or `manage`)
- `GET /auth/directory-grants` returns the current user's entry directories

Directory grants inherit down the directory tree. Administrators bypass
directory ACLs. Non-administrators must specify a permitted directory when
listing resources. Direct resource reads, previews, downloads, actions, and
updates are checked against the resource's directory. Creates, uploads, scans,
moves, deletes, and plugin write actions are authorized inside the core use
case after their target directory is known.

Storage scanning is an administrator-only maintenance operation. It skips
symbolic links, stops after 100,000 filesystem entries, and does not calculate
SHA-256 unless the request explicitly sets `sha256: true`.

The identity and authorization model lives in `asset-core`: `User`,
`AccessContext`, `DirectoryGrant`, and `DirectoryPermission` are domain types;
user, password, and access-policy persistence are ports. `asset-infra` supplies
the SQLite and Argon2 adapters. HTTP only maps the authenticated session into an
`AccessContext` and calls the secured resource service, so future CLI and TUI
entry points can reuse the same authorization rules.

## HTTP Boundary Settings

These environment variables control the HTTP shell around the API:

- `ASSET_HTTP_ADDR`: listen address, default `127.0.0.1:8080`.
- `ASSET_HTTP_ENABLE_SWAGGER`: expose `/swagger-ui` and `/api-docs/openapi.json`, default `true`.
- `ASSET_HTTP_ENABLE_PURGE`: expose physical delete endpoint `DELETE /resources/{id}/purge`, default `true` for local development.
- `ASSET_HTTP_CORS_ALLOWED_ORIGINS`: comma-separated explicit origins. Wildcards are rejected because authentication uses cookies.
- `ASSET_HTTP_REQUEST_TIMEOUT_SECS`: request timeout, default `30`.
- `ASSET_HTTP_COOKIE_SECURE`: mark the session cookie Secure. Set this to `true` whenever the public URL uses HTTPS; default `false` for local HTTP.
- `ASSET_HTTP_SESSION_INACTIVITY_SECS`: session inactivity lifetime, default `43200` (12 hours).

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

The JSON/base64 upload endpoint is limited to 32 MiB of decoded content. The
admin UI uses the streaming upload endpoint, which supports files up to 4 GiB.

## Docker

The repository includes production-oriented API and Web image targets plus a
Compose stack:

```bash
cp docker/.env.example docker/.env
# Edit docker/.env and replace the bootstrap administrator password.
cd docker
docker compose up -d --build
```

Open `http://127.0.0.1:8080`. SQLite data, sessions, ACLs, and uploaded blobs
are persisted in the `asset-hub-data` Docker volume. See
[`docker/README.md`](docker/README.md) for all required settings, image-only
commands, plugin configuration, upgrades, backups, restores, and production
hardening.

To point the UI at a different API origin:

```bash
VITE_API_BASE_URL=http://127.0.0.1:8080 npm run dev
```

Also add the UI origin to `ASSET_HTTP_CORS_ALLOWED_ORIGINS`. Credentialed CORS only accepts explicit origins. A same-origin reverse proxy remains the recommended deployment.

## Plugins

Plugin manifest paths are configured under `[kind].plugin_manifests` in `config.toml`.
Manifest paths are loaded at API startup; restart the service after changing plugin manifests or WASM files.

Resource kinds form an arbitrary-depth acyclic hierarchy through the optional
`parent` field. Child kinds inherit actions and the nearest metadata schema from
their ancestors; their own declarations override inherited values. Detection
returns the most specific matching kind. For example, the bundled hierarchy
contains `core:file → core:document → azvs:markdown`; `core:unknown` is another
child of `core:file` for files whose concrete format has not been identified.

Kind-filtered list endpoints accept `include_descendants=true`. A query for
`core:document` can therefore include Markdown, EPUB, source-code families, and
any future nested document formats.
Plugin calls are limited to 64 MiB of resource input, 256 MiB of WASM linear
memory, and 20 seconds of execution time. Larger assets remain downloadable but
cannot be passed to the current in-memory plugin ABI.

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

Run the local checks:

- `cargo test --workspace`
- `cargo test --manifest-path plugins/azvs-epub/Cargo.toml`
- `cargo test --manifest-path plugins/azvs-mp4/Cargo.toml`
- `npm run lint` and `npm run build` in `asset-web-admin`
- `npm run typecheck` and `npm run build` in `plugins/azvs-markdown/web`
- `cargo test --manifest-path plugins/azvs-markdown/plugin/Cargo.toml`

Individual checks:

```bash
cargo test --workspace
cd asset-web-admin && npm run lint && npm run build
cd asset-web-admin && npm test
cd plugins/azvs-markdown/web && npm run typecheck && npm run build
cargo test --manifest-path plugins/azvs-markdown/plugin/Cargo.toml
```
