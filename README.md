# Asset Hub

Asset Hub is a local-first asset management service. The current workspace contains:

- `asset-core`: workspace-internal resource domain model, adapter ports, and secured resource use cases.
- `asset-infra`: SQLite repository, OpenDAL Fs blob storage, built-in kinds, plugin manifest loading, and Extism action execution.
- `asset-http`: complete HTTP application, including its executable entry point, router, handlers, and host wiring.
- `asset-cli`: complete administration CLI, including its executable entry point, commands, and on-demand runtime wiring.
- `asset-runtime`: shared runtime assembly that owns initialized infrastructure and background-task lifetimes.
- `asset-plugin-api`: shared manifest, action, request, and view contracts for plugins.
- `asset-web`: React host with domain/application/adapter boundaries and a slot-based plugin kernel.
- `plugins`: sample Markdown and EPUB plugins.

## API Boundaries

`asset-plugin-api` is the only extension contract for plugin authors. Plugin runtimes must not
depend on `asset-core`, `asset-infra`, or `asset-runtime`; those crates are host implementation details
and do not carry a compatibility promise for external consumers.

Inside the host workspace, `asset-core` exposes three deliberately separate surfaces:

- `domain` contains validated business values and aggregates shared with applications and adapters.
- `port` contains the curated SPI implemented by repositories, object storage, scanners, and the
  plugin host. Port implementation modules are private, so every SPI type has one import path.
- `service` contains application commands and results. Untrusted HTTP, CLI, and TUI entry points
  bind an `AccessContext` through `ResourceService::secured`; unbound command, content, and action
  services are Core implementation details.

`asset-http` and `asset-cli` each own their executable startup and may use `asset-infra` when they need
concrete configuration or infrastructure behavior. Protocol handlers and commands should still prefer
`asset-core` services. Shared runtime assembly and background-task ownership live in `asset-runtime`.

Manifest, action, request, view, diagnostic, and Host function types belong to `asset-plugin-api` and
are imported from that crate directly rather than re-exported through `asset-core`.
The Rust crate and Manifest document are versioned independently from the unified Plugin API.
The Plugin API covers Action JSON, Wasm Host functions, and Plugin Frame messages; compatibility
and release rules are documented in
[`asset-plugin-api/README.md`](asset-plugin-api/README.md).

## Requirements

- Rust 1.97.1 (pinned by `rust-toolchain.toml`).
- Node.js 22.22.2 (pinned by `.node-version`; compatible with fnm) and npm 10.

## Run The API

Start the HTTP API from the repository root:

```bash
cargo run -p asset-http --bin asset-http
```

Defaults:

- API listens on `127.0.0.1:8080`.
- SQLite database is `data/.asset-hub/asset-hub.sqlite`.
- Plugins are discovered from `data/.asset-hub/plugins/<plugin-id>`.
- File storage root is `data`.
- If `config.toml` exists, it is loaded. Otherwise built-in defaults are used.

Use a config file:

```bash
cargo run -p asset-http --bin asset-http -- --config config.example.toml
```

The administration CLI exposes configuration inspection and local user-management commands, with
`system` and `plugin` retained as extension points:

```bash
cargo run -p asset-cli --bin asset -- --help
```

## Users And Directory Access

The HTTP API uses SQLite-backed login sessions. On the first startup, provide an
initial administrator; the password is stored as an Argon2 hash and is never
written to configuration:

```bash
ASSET_HUB_BOOTSTRAP_ADMIN_USERNAME=admin \
ASSET_HUB_BOOTSTRAP_ADMIN_PASSWORD='replace-with-a-long-password' \
cargo run -p asset-http --bin asset-http
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
content reads, downloads, actions, and updates are checked against the
resource's directory. Creates, uploads, moves, deletes, and plugin write
actions are authorized inside the core use case after their target directory is
known.

When user creation omits `workspace_directory`, the user service assigns
`users/<username>` to members and `/` to administrators. API clients can still provide an
explicit workspace when a custom boundary is required.

Directory is an independent aggregate rather than a path embedded in Resource.
Every directory has a stable UUID, a direct `parent_id`, a kind, and lifecycle
timestamps. SQLite is the source of truth and stores the hierarchy as adjacency
data. At startup the host loads all directory aggregates into a rebuildable
`InMemoryDirectoryIndex`, which derives paths and tree projections and is kept in
sync after committed writes. Resource and User rows reference `directory_id`;
paths remain HTTP and Blob-storage projections, so renaming or moving a directory
does not invalidate domain references. The fixed nil UUID is the persisted global
root directory.

The process-local index assumes a single service instance owns directory writes.
For multi-instance deployment it must be replaced by a coherently invalidated
shared index (or rebuilt from a change stream); using Redis only as a startup copy
without invalidation would still permit stale authorization and path projections.

Security events are stored in SQLite's `security_audit_events` table. Login
successes, login failures, rate-limit rejections, and classified state-changing
operations are recorded with a source-independent event type, operation source,
and outcome. Protocol-specific diagnostics remain in application logs instead of
expanding the audit table. Unauthenticated operations keep the actor ID empty;
unverified login input is stored only as the event target. Request bodies,
passwords, tokens, and other secret CLI arguments are never stored. Audit
persistence is fail-open: a temporary audit write error is logged without
replacing the business response.

Local Blob storage is synchronized automatically. When an existing Resource index is available,
startup reconciliation uses persisted file modification times and content sizes, hashing only new
or changed files. Run
`asset system --scan-resource` for an explicit full SHA-256 verification. Native file-system events are
debounced for near-real-time updates, while startup and periodic reconciliation
repair changes missed while the process was stopped. New and modified files
update Resource content and checksums, external deletes remove the corresponding
database record, directory changes synchronize the directory table, and the
reserved `.asset-hub` namespace is always ignored.

Plugin write actions can update object bytes through `replace_content`. Runtime
errors are compensated, but the current OpenDAL-backed replacement flow is not a
crash-safe transaction across SQLite and object storage: if the process or
machine stops after the object bytes are replaced but before the database row is
updated, the database can temporarily or permanently record stale size/checksum
values for that storage key. Detection and repair for this condition are deferred
to a future explicit maintenance design.

The directory, identity, and authorization models live in `asset-core`:
`Directory`, `User`, `AccessContext`, and `DirectoryPermission` are domain
types; their persistence and physical-directory operations are ports.
`asset-infra` supplies the SQLite, local storage, and Argon2 adapters.
HTTP only maps the authenticated session into an
`AccessContext` and calls the secured resource service, so future CLI and TUI
entry points can reuse the same authorization rules.

## HTTP Boundary Settings

`asset-http` uses Clap for command-line parsing. Run
`cargo run -p asset-http --bin asset-http -- --help` to see all options. Each
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
cargo run -p asset-http --bin asset-http -- \
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

Uploads use persistent sessions and 8 MiB client-side chunks. Before creating a session, a Web
Worker incrementally calculates the local file SHA-256 without buffering the complete file. The
digest is persisted as the expected checksum and is part of the resume fingerprint, so a different
same-name/same-size file cannot attach to an older session. Interrupted uploads resume from the
server-reported offset after the user selects the same file again. Every `PATCH` includes an
independent `Upload-Checksum` SHA-256. The server streams that request into an isolated temporary
chunk while hashing it, and only appends verified bytes to the session's staged file. A mismatch
deletes the temporary chunk and leaves the durable upload offset unchanged. The web client retries
an explicitly rejected checksum up to three times before surfacing the resumable failure.
Once all bytes arrive,
`POST /uploads/{id}/complete` persists the `finalizing` state and returns `202 Accepted`; the
server-side SHA-256 must match the expected checksum before atomic publication and Resource
creation. A mismatch marks the session failed and never publishes the staged file.
Clients poll `GET /uploads/{id}` until it reports `completed` or `failed`. Pending finalizations
resume automatically after a service restart. Background checksum verification is currently
unbounded, so separate uploads may finalize concurrently. The regular request timeout does not cap
upload chunks; deployments should use proxy-level connection and idle timeouts to reject stalled
clients without terminating healthy uploads. The API does not impose a per-file size limit, and
neither hashing nor uploading buffers the complete file in application memory.

When automatic local-storage synchronization is enabled, the filesystem watcher is established
before startup returns. After the Resource database has been recreated, recovery runs in two
stages: a metadata-only scan first creates real Resources with `pending` content verification, then
one independent background task per StorageKey calculates SHA-256 and changes the state to
`verified` or `failed`. The metadata stage does not read complete object bodies, so Resources become
available without waiting for large files to be hashed. Background verification has no global
concurrency limit; StorageKey locks only serialize work targeting the same object.

## Package A Plugin

Author a Manifest V1 package whose directory name equals `plugin.id`. Extism packages use the
fixed `plugin.wasm` entry; optional Web UI starts at the package-root `index.html`. Install the
package without a lock file. On first startup Asset Hub calculates every plugin artifact digest and
atomically creates `manifest.lock.json`. On later startups the existing lock is only verified and
is never refreshed automatically. The service therefore needs write access to a newly installed
plugin directory for its first startup.

After the first successful startup, the generated package can be checked without modifying it:

```bash
cargo run -p asset-cli --bin asset -- \
  plugin --verify .asset-hub/plugins/example.tools/manifest.json
```

To deploy a changed plugin, replace the package as a new lock-free installation rather than
retaining its previous lock.

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

Plugins are discovered from `<blob.local.root>/.asset-hub/plugins/<plugin-id>/manifest.json`.
The package directory must exactly match `plugin.id`; no `[kind]` configuration is required.
Manifests, Wasm, and Web assets are read once at API startup. A missing lock is generated
atomically; an existing lock is integrity-checked without modification. Wasm is
precompiled and every declared handler export is checked before the server starts; Web assets are
served from the verified in-memory snapshot. Restart the service after changing any plugin file.

All plugin execution limits are assembled once from `[plugin]` into the
`asset-plugin-api::PluginExecutionPolicy` shared by the core action service and Wasm host. The
policy covers total content size, inline content size, per-read chunk size, serialized input and
output size, concurrency, Wasm memory, and execution timeout. A configured
`plugin.max_content_bytes` is therefore the effective limit in both layers; there is no separate
core content ceiling.

Non-inline Wasm content is passed as an opaque, call-scoped reference rather than file bytes. The
Plugin API exposes `asset_hub_content_open`, `asset_hub_content_size`,
`asset_hub_content_read`, and `asset_hub_content_close`. The `extism-guest` feature of
`asset-plugin-api` provides the safe client used by bundled plugins. `content_read` returns raw
bytes for the requested offset and may return a smaller chunk according to
`plugin.max_content_read_bytes`, so plugins must continue reading until `content_size` is reached.
Handles cannot be reused by another plugin call and are reclaimed automatically when a call ends.
This keeps large input content out of JSON and avoids Base64 expansion; inline content remains
available for small payloads controlled by `plugin.max_inline_content_bytes`.
External Wasm plugins declare `content_delivery` as `reference`, read bounded ranges, rebuild their
Wasm, and reinstall the package without carrying over its previous lock.

Resource kinds form an arbitrary-depth acyclic hierarchy through the optional
`parent` field. Child kinds inherit actions, and their own action declarations
override inherited actions with the same ID. `tags` are direct fields of every
Resource. Detection returns the most specific
matching kind. For example, the bundled hierarchy contains
`core:resource → core:document → azvs:markdown`. `core:resource` is the root and the
default for resources whose concrete format has not been identified.
Directory kinds use the separate `capabilities.directory_kinds` manifest capability
and an independent acyclic registry rooted at the built-in `core:directory`.

Kind-filtered list endpoints include all descendant kinds by default. A query for
`core:document` therefore includes Markdown, EPUB, source-code families, and any
future nested document formats.
By default, plugin calls are limited to 64 MiB of resource content, 4 MiB inline content and read
chunks, 8 MiB of serialized input, 8 MiB of output, 256 MiB of Wasm linear memory, 20 seconds, and
eight concurrent calls. The HTTP action input itself is limited to 1 MiB. These values are
configured under `[plugin]`; non-inline content up to the configured total limit is available
through the Range-based handle API.

Manifest V1 uses a fine-grained `permissions.allow` list containing
`resource.read`, `resource.write`, `resource.content.read`, `resource.content.replace`, and
`resource.derived_asset.write`. Effects are checked against their specific permission. Network and
filesystem permissions are requested by a Manifest but are not self-granting. Every
requested host or path must also be approved under `[plugin.grants]`. Network grants are exact and
wildcards are rejected; filesystem grants are normalized roots. The default host policy grants no
network or filesystem access.

Resource and directory actions are declared independently under
`capabilities.resource_actions` and `capabilities.directory_actions`.

The current and only supported authoring target is Manifest V1 with
`runtime.plugin_api = "asset-hub.plugin-api@1"`. Both versions must be declared explicitly;
other manifest or Plugin API versions are rejected at startup. Plugin failures
may return a structured `error` diagnostic with a stable `code`, message, retry hint, and optional
JSON details; successful outputs may also carry non-fatal diagnostics.

An initial package contains `manifest.json`, plus `plugin.wasm` for an Extism runtime and
`index.html` with any relative asset layout for a Web UI. Asset Hub creates `manifest.lock.json`
on first startup. The lock contains one flat `integrity` map keyed by package-relative file path;
`manifest.json` and the lock itself are excluded. Non-Wasm entries in that verified map are exposed
over HTTP.

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
