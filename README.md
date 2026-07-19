# Asset Hub

Asset Hub is a local-first asset management service. The current workspace contains:

- `asset-core`: workspace-internal resource domain model, adapter ports, and secured resource use cases.
- `asset-infra`: SQLite repository, OpenDAL Fs blob storage, built-in kinds, plugin manifest loading, and Extism action execution.
- `asset-apps`: reusable runtime assembly plus the `asset-http` API, `asset` administration CLI,
  and the existing `asset-plugin` packaging CLI.
- `asset-plugin-api`: shared manifest, action, request, and view contracts for plugins.
- `asset-web`: React host with domain/application/adapter boundaries and a slot-based plugin kernel.
- `plugins`: sample Markdown and EPUB plugins.

## API Boundaries

`asset-plugin-api` is the only extension contract for plugin authors. Plugin runtimes must not
depend on `asset-core`, `asset-infra`, or `asset-apps`; those crates are host implementation details
and do not carry a compatibility promise for external consumers.

Inside the host workspace, `asset-core` exposes three deliberately separate surfaces:

- `domain` contains validated business values and aggregates shared with applications and adapters.
- `port` contains the curated SPI implemented by repositories, object storage, scanners, and the
  plugin host. Port implementation modules are private, so every SPI type has one import path.
- `service` contains application commands and results. Untrusted HTTP, CLI, and TUI entry points
  bind an `AccessContext` through `ResourceService::secured`; unbound command, content, action, and
  preview services are Core implementation details.

Manifest, action, request, view, diagnostic, and content ABI types belong to `asset-plugin-api` and
are imported from that crate directly rather than re-exported through `asset-core`.
The Rust crate, Manifest document, plugin JSON API, and content ABI are versioned independently;
their compatibility and release rules are documented in
[`asset-plugin-api/README.md`](asset-plugin-api/README.md).

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

The administration CLI currently exposes empty `config`, `system`, `user`, and `plugin` command
groups as extension points:

```bash
cargo run -p asset-apps --bin asset -- --help
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

Failed logins are limited to five attempts per username in a 60-second window.
The in-memory limiter uses fixed-size username digests, expires inactive entries,
and keeps at most 10,000 entries, so arbitrary unauthenticated usernames cannot
grow the table without bound. Login JSON bodies are capped at 16 KiB. A
successful login clears that username's failure state.

Authentication endpoints:

- `POST /auth/login` with `{ "username": "...", "password": "..." }`
- `POST /auth/logout`
- `GET /auth/me`
- `GET /auth/audit-events?page=1&limit=100` (administrator only, limit is
  clamped to 1-500)
- `POST /auth/users` (administrator only)

Administrators can access all user-visible directories. A non-administrator's
`workspace_directory` is their only authorization boundary: they have complete
access to that directory and its descendants, and no access outside it. Direct resource
reads, previews, downloads, actions, and updates are checked against the
resource's directory. Creates, uploads, scans, moves, deletes, and plugin write
actions are authorized inside the core use case after their target directory is
known.

Security events are stored in SQLite's `security_audit_events` table. Login
successes, login failures, rate-limit rejections, and classified state-changing
operations are recorded with a source-independent event type, operation source,
and outcome. Protocol-specific diagnostics remain in application logs instead of
expanding the audit table. Unauthenticated operations keep the actor ID empty;
unverified login input is stored only as the event target. Request bodies,
passwords, tokens, and other secret CLI arguments are never stored. Audit
persistence is fail-open: a temporary audit write error is logged without
replacing the business response.

Storage scanning is an administrator-only maintenance operation. `POST /scan`
imports previously unknown files and empty directories from the configured
storage root. It skips symbolic links, stops after 100,000 filesystem entries,
and does not calculate file checksums. Its request scope is the object-key
`prefix`; the legacy `directory` request field remains accepted as an alias but
does not represent a logical resource directory.

Plugin write actions can update object bytes through `replace_content`. Runtime
errors are compensated, but the current OpenDAL-backed replacement flow is not a
crash-safe transaction across SQLite and object storage: if the process or
machine stops after the object bytes are replaced but before the database row is
updated, the database can temporarily or permanently record stale size/checksum
values for that storage key. Detection and repair for this condition are deferred
to a future explicit maintenance design.

The identity and authorization model lives in `asset-core`: `User`,
`AccessContext`, and `DirectoryPermission` are domain types; user and password
persistence are ports. `asset-infra` supplies the SQLite and Argon2 adapters.
HTTP only maps the authenticated session into an
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
- `--request-timeout-secs` / `ASSET_HTTP_REQUEST_TIMEOUT_SECS`: total timeout for regular requests, default `30`; streaming uploads are exempt.
- `--cookie-secure` / `ASSET_HTTP_COOKIE_SECURE`: mark the session cookie Secure. Enable this whenever the public URL uses HTTPS; default `false` for local HTTP.
- `--session-inactivity-secs` / `ASSET_HTTP_SESSION_INACTIVITY_SECS`: session inactivity lifetime, default `43200` (12 hours).

Boolean options accept an omitted value as `true`, or an explicit value such
as `--enable-swagger=false`. Bootstrap credentials remain environment-only so
passwords are not encouraged in process arguments.

`GET /health` is an unauthenticated readiness endpoint. It checks both SQLite
and the configured blob-storage namespace, reports each component as `ready` or
`unavailable`, and returns `503 Service Unavailable` if either dependency is
not ready.

Example production-leaning local run:

```bash
cargo run -p asset-apps --bin asset-http -- \
  --enable-swagger=false \
  --enable-purge=false \
  --cors-allowed-origins http://127.0.0.1:5173
```

## Run The Web UI

In one terminal, start the API. In another:

```bash
cd asset-web
npm install
npm run dev
```

Vite serves `http://127.0.0.1:5173` and proxies `/api` to `http://127.0.0.1:8080`.

Uploads use the raw-body streaming endpoint, which supports files up to 4 GiB
without encoding them as base64 or buffering the complete file in application memory. The regular
request timeout does not cap total upload duration; deployments should use proxy-level connection
and idle timeouts to reject stalled clients without terminating healthy long-running uploads.

## Package A Plugin

Generate a Manifest V3 starter in a new plugin directory:

```bash
asset-plugin gen manifest
```

Generate the matching Draft 2020-12 JSON Schema for editor or CI integration with
`asset-plugin gen schema`.

This creates `manifest.json` without overwriting an existing file. Replace the `example.plugin`
identity, action, handler, matching rules, requirements, views, and permissions, then build the
Wasm and optional Web bundle. The source template is
`asset-plugin-api/templates/manifest.json`, so protocol template changes require no CLI code
changes. Integrity data is generated; seal the finished artifacts:

```bash
cargo run -p asset-apps --bin asset-plugin -- \
  seal path/to/plugin.json
```

The command calculates the Wasm digest and complete Web asset map into a sibling
`manifest.lock.json`, then runs plugin contract validation. Do not run `seal` during
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

All plugin execution limits are assembled once from `[plugin]` into the
`asset-plugin-api::PluginExecutionPolicy` shared by the core action service and Wasm host. The
policy covers total content size, inline content size, per-read chunk size, serialized input and
output size, concurrency, Wasm memory, and execution timeout. A configured
`plugin.max_content_bytes` is therefore the effective limit in both layers; there is no separate
core content ceiling.

Non-inline Wasm content is passed as an opaque, call-scoped reference rather than file bytes. The
versioned content ABI exposes `asset_hub_content_open`, `asset_hub_content_size`,
`asset_hub_content_read`, and `asset_hub_content_close`. The `extism-guest` feature of
`asset-plugin-api` provides the safe client used by bundled plugins. `content_read` returns raw
bytes for the requested offset and may return a smaller chunk according to
`plugin.max_content_read_bytes`, so plugins must continue reading until `content_size` is reached.
Handles cannot be reused by another plugin call and are reclaimed automatically when a call ends.
This keeps large input content out of JSON and avoids Base64 expansion; inline content remains
available for small payloads controlled by `plugin.max_inline_content_bytes`.
This host ABI replaces the former single-argument, Base64-returning `asset_hub_content_read`.
External Wasm plugins declare `content_delivery` as `reference`, read bounded ranges, rebuild their
Wasm, and reseal the package.

Resource kinds form an arbitrary-depth acyclic hierarchy through the optional
`parent` field. Child kinds inherit actions, and their own action declarations
override inherited actions with the same ID. `description` and `tags` are direct
fields of every Resource. Detection returns the most specific
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

Manifest V3 uses a fine-grained `permissions.allow` list containing
`resource.read`, `resource.write`, `content.read`, `content.replace`, and
`derived_asset.write`. Effects are checked against their specific permission. Network and
filesystem permissions are requested by a Manifest but are not self-granting. Every
requested host or path must also be approved under `[plugin.grants]`. Network grants are exact and
wildcards are rejected; filesystem grants are normalized roots. The default host policy grants no
network or filesystem access.

The current authoring target is Manifest V3 with plugin API `0.3`; omitting `runtime.plugin_api`
selects it. The host continues to accept Manifest V2, but plugin JSON API `0.2` is the only
supported protocol level. Other manifest or ABI versions are rejected at startup. Plugin failures
may return a structured `error` diagnostic with a stable `code`, message, retry hint, and optional
JSON details; successful outputs may also carry non-fatal diagnostics.

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
- `npm run check`, `npm test`, and `npm run build` in `asset-web`
- `npm run typecheck` and `npm run build` in `plugins/azvs-markdown/web`
- `npm run typecheck` and `npm run build` in `plugins/azvs-epub/web`

Individual checks:

```bash
cargo test --workspace
(cd asset-web && npm run check && npm test && npm run build)
(cd plugins/azvs-markdown/web && npm run typecheck && npm run build)
cargo test --manifest-path plugins/azvs-markdown/runtime/Cargo.toml
cargo test --manifest-path plugins/azvs-epub/runtime/Cargo.toml
(cd plugins/azvs-epub/web && npm run typecheck && npm run build)
```
