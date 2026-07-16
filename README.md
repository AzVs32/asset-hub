# Asset Hub

Asset Hub is a local-first asset management service. The current workspace contains:

- `asset-core`: resource domain model, repository/storage/action ports, and resource service.
- `asset-infra`: SQLite repository, OpenDAL Fs blob storage, built-in kinds, plugin manifest loading, and Extism action execution.
- `asset-apps`: reusable runtime assembly plus the `asset-http` API and `asset-plugin` packaging CLI.
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
- SQLite database is `data/.asset-hub/asset-hub.sqlite`.
- File storage root is `data`.
- If `config.toml` exists, it is loaded. Otherwise built-in defaults are used.

Use a config file:

```bash
cargo run -p asset-apps --bin asset-http -- --config config.example.toml
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
  `directory`, and resource `permission` (`read`, `write`, or `full`)
- `GET /auth/directory-grants` returns the complete entry-directory set and
  marks the user's shared-capable `workspace_directory`

Directory grants inherit down the directory tree. Administrators bypass
directory ACLs. Every user is created with an explicit `full` grant for their
workspace; the workspace itself does not imply permission. Non-administrators
must specify a permitted directory when listing resources. Direct resource
reads, previews, downloads, actions, and updates are checked against the
resource's directory. Creates, uploads, scans, moves, deletes, and plugin write
actions are authorized inside the core use case after their target directory is
known.

Storage scanning and auditing are administrator-only maintenance operations.
`POST /scan` imports previously unknown files from the configured storage root.
`POST /audit` is read-only: it compares resource content rows with objects under
the storage root and reports missing blobs, size/checksum mismatches, and orphan
objects. Both operations skip symbolic links, stop after 100,000 filesystem
entries, and are limited to administrators. Scan does not calculate SHA-256
unless the request explicitly sets `sha256: true`; audit calculates SHA-256 by
default and accepts `sha256: false` when only existence and size checks are
needed.

Plugin write actions can update object bytes through `replace_content`. Runtime
errors are compensated, but the current OpenDAL-backed replacement flow is not a
crash-safe transaction across SQLite and object storage: if the process or
machine stops after the object bytes are replaced but before the database row is
updated, the database can temporarily or permanently record stale size/checksum
metadata for that storage key. Use `POST /audit` to detect that condition. Audit
does not repair data; any repair flow should be introduced as a separate,
explicit maintenance operation.

The identity and authorization model lives in `asset-core`: `User`,
`AccessContext`, `DirectoryGrant`, and `DirectoryPermission` are domain types;
user, password, and access-policy persistence are ports. `asset-infra` supplies
the SQLite and Argon2 adapters. HTTP only maps the authenticated session into an
`AccessContext` and calls the secured resource service, so future CLI and TUI
entry points can reuse the same authorization rules.

## HTTP Boundary Settings

`asset-http` uses Clap for command-line parsing. Run
`cargo run -p asset-apps --bin asset-http -- --help` to see all options. Each
HTTP option also accepts its existing environment variable for deployment
compatibility; an explicit command-line value takes precedence over the
environment:

- `--config` / `ASSET_HUB_CONFIG`: Asset Hub TOML configuration file.
- `--addr` / `ASSET_HTTP_ADDR`: listen address, default `127.0.0.1:8080`.
- `--enable-swagger` / `ASSET_HTTP_ENABLE_SWAGGER`: expose `/swagger-ui` and `/api-docs/openapi.json`, default `true`.
- `--enable-purge` / `ASSET_HTTP_ENABLE_PURGE`: expose physical delete endpoint `DELETE /resources/{id}/purge`, default `true` for local development.
- `--cors-allowed-origins` / `ASSET_HTTP_CORS_ALLOWED_ORIGINS`: comma-separated explicit origins. Wildcards are rejected because authentication uses cookies.
- `--request-timeout-secs` / `ASSET_HTTP_REQUEST_TIMEOUT_SECS`: request timeout, default `30`.
- `--cookie-secure` / `ASSET_HTTP_COOKIE_SECURE`: mark the session cookie Secure. Enable this whenever the public URL uses HTTPS; default `false` for local HTTP.
- `--session-inactivity-secs` / `ASSET_HTTP_SESSION_INACTIVITY_SECS`: session inactivity lifetime, default `43200` (12 hours).

Boolean options accept an omitted value as `true`, or an explicit value such
as `--enable-swagger=false`. Bootstrap credentials remain environment-only so
passwords are not encouraged in process arguments.

Example production-leaning local run:

```bash
cargo run -p asset-apps --bin asset-http -- \
  --enable-swagger=false \
  --enable-purge=false \
  --cors-allowed-origins http://127.0.0.1:5173
```

## Run The Admin UI

In one terminal, start the API. In another:

```bash
cd asset-web-admin
npm install
npm run dev
```

Vite serves `http://127.0.0.1:5173` and proxies `/api` to `http://127.0.0.1:8080`.

Uploads use the raw-body streaming endpoint, which supports files up to 4 GiB
without encoding them as base64 or buffering the complete file in application memory.

## Package A Plugin

Generate a fixed Manifest V2 starter in a new plugin directory:

```bash
asset-plugin gen manifest
```

This creates `manifest.json` without overwriting an existing file. Replace the `example.plugin`
metadata, action, handler, matching rules, requirements, views, and permissions, then build the
Wasm and optional Web bundle. The source template is
`asset-plugin-api/templates/manifest.json`, so protocol template changes require no CLI code
changes. Integrity data is generated; seal the finished artifacts:

```bash
cargo run -p asset-apps --bin asset-plugin -- \
  seal path/to/plugin.json
```

The command calculates the Wasm digest and complete Web asset map into a sibling
`manifest.lock.json`, then runs Manifest V2 contract validation. Do not run `seal` during
application startup: release or CI should instead verify the previously sealed package without
modifying it:

```bash
cargo run -p asset-apps --bin asset-plugin -- \
  verify path/to/plugin.json
```

The standalone binary can also be installed with
`cargo install --path asset-apps --bin asset-plugin`, after which the equivalent commands are
`asset-plugin gen manifest`, `asset-plugin seal ...`, and `asset-plugin verify ...`.

## Docker

The repository includes production-oriented API and Web image targets plus a
Compose stack:

```bash
cp docker/.env.example docker/.env
# Edit docker/.env and replace the bootstrap administrator password.
cd docker
docker compose up -d --build
```

Open `http://127.0.0.1:8080`. Runtime data and uploaded files are persisted in
the `asset-hub-data` Docker volume; container configuration is persisted in the
`asset-hub-conf` Docker volume. See
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
Manifests, Wasm, and Web assets are read and integrity-checked once at API startup. Wasm is
precompiled and every declared handler export is checked before the server starts; Web assets are
served from the verified in-memory snapshot. Restart the service after changing any plugin file.

All plugin execution limits are assembled once from `[plugin]` into a shared
`PluginExecutionPolicy` and injected into both the core action service and the Wasm host. The
policy covers total content size, inline content size, per-read chunk size, serialized input and
output size, concurrency, Wasm memory, and execution timeout. A configured
`plugin.max_content_bytes` is therefore the effective limit in both layers; there is no separate
core content ceiling.

Non-inline Wasm content is passed as an opaque, call-scoped reference rather than file bytes. A
plugin reads it through the `asset_hub_content_open`, `asset_hub_content_size`,
`asset_hub_content_read`, and `asset_hub_content_close` host functions. `content_read` returns raw
bytes for the requested offset and may return a smaller chunk according to
`plugin.max_content_read_bytes`, so plugins must continue reading until `content_size` is reached.
Handles cannot be reused by another plugin call and are reclaimed automatically when a call ends.
This keeps large input content out of JSON and avoids Base64 expansion; inline content remains
available for small payloads controlled by `plugin.max_inline_content_bytes`.
This host ABI replaces the former single-argument, Base64-returning `asset_hub_content_read`.
External Wasm plugins must declare `content_delivery` as `reference`, migrate to the handle
functions, rebuild their Wasm, and reseal the package; startup preflight rejects the old import
signature.

Resource kinds form an arbitrary-depth acyclic hierarchy through the optional
`parent` field. Child kinds inherit actions, and their own action declarations
override inherited actions with the same ID. Resource metadata is not defined
by kinds: all resources currently use the strict, versioned core summary schema
containing `description` and `tags`. Detection returns the most specific
matching kind. For example, the bundled hierarchy contains
`core:file → core:document → azvs:markdown`; `core:unknown` is another child of
`core:file` for files whose concrete format has not been identified.

Kind-filtered list endpoints accept `include_descendants=true`. A query for
`core:document` can therefore include Markdown, EPUB, source-code families, and
any future nested document formats.
By default, plugin calls are limited to 64 MiB of resource content, 4 MiB inline content and read
chunks, 8 MiB of serialized input, 8 MiB of output, 256 MiB of Wasm linear memory, 20 seconds, and
eight concurrent calls. The HTTP action input itself is limited to 1 MiB. These values are
configured under `[plugin]`; non-inline content up to the configured total limit is available
through the Range-based handle API.

Network and filesystem permissions are requested by a Manifest but are not self-granting. Every
requested host or path must also be approved under `[plugin.grants]`. Network grants are exact and
wildcards are rejected; filesystem grants are normalized roots. The default host policy grants no
network or filesystem access.

Example:

```toml
[kind]
plugin_manifests = [
  "plugins/azvs-markdown/manifest.json",
  "plugins/azvs-epub/manifest.json",
]
```

## Checks

Run the local checks:

- `cargo test --workspace`
- `cargo test --manifest-path plugins/azvs-markdown/runtime/Cargo.toml`
- `cargo test --manifest-path plugins/azvs-epub/runtime/Cargo.toml`
- `npm run lint` and `npm run build` in `asset-web-admin`
- `npm run typecheck` and `npm run build` in `plugins/azvs-markdown/web`
- `npm run typecheck` and `npm run build` in `plugins/azvs-epub/web`

Individual checks:

```bash
cargo test --workspace
(cd asset-web-admin && npm run lint && npm run build && npm test)
(cd plugins/azvs-markdown/web && npm run typecheck && npm run build)
cargo test --manifest-path plugins/azvs-markdown/runtime/Cargo.toml
cargo test --manifest-path plugins/azvs-epub/runtime/Cargo.toml
(cd plugins/azvs-epub/web && npm run typecheck && npm run build)
```
