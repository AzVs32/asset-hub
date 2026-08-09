# Asset HTTP

`asset-http` is the Axum transport and the composition root for HTTP-only policy. It owns routing,
cookie authentication, OpenAPI, request limits, CORS, and the authentication-session store.

Business handlers receive Core application services. Resource and Directory kind definitions are
queried through `ResourceService`/`DirectoryService`, not through kind-registry Ports obtained from
`AssetRuntime`. Upload completion receives the narrow `UploadFinalizationDispatcher` Host
capability; HTTP does not depend on the concrete Runtime scheduler or supervisor.

Resource and Directory contracts deliberately use the same shape where their semantics overlap.
Both expose stable UUIDs, kind definitions with typed `origin` metadata, flattened action arrays,
and monotonically increasing `revision` values. Directory creation accepts a stable `parent_id`;
`GET`, `PATCH`, and `DELETE /directories/{id}` address the aggregate by UUID. Mutating Resource and
Directory requests and write Actions require `expected_revision` (streaming content replacement
uses `If-Match`) and return a coded revision conflict when another writer has advanced the
aggregate. Read Actions omit the precondition by default and run against the latest authorized
snapshot; callers may still supply it when exact snapshot consistency is required. Path strings
remain navigation and display data, not Directory identity.

Kind-list responses retain their contextual action declarations, while Resource and Directory
responses contain actions that are actually applicable to that aggregate and content state. This
small amount of metadata repetition is intentional: aggregate responses remain self-contained and
clients do not need to join a global catalog before rendering or executing an action.

## Session storage boundary

HTTP login sessions use a dedicated SQLite file and connection pool. The store never receives a
pool, path, URL, or SQLx type from `asset-runtime` or `asset-infra`; the business database therefore
remains free to evolve independently. `upload_sessions` are a Core business aggregate and are not
part of this HTTP authentication-session store.

The executable uses the fixed relative path `data/.asset-hub/http-session.sqlite`, resolved from
its working directory. Its pool size is fixed at 5 connections and expired sessions are deleted
once per hour. These adapter implementation details are intentionally absent from `config.toml`,
CLI flags and environment variables. Cookie policy remains configurable with:

| CLI option | Default | Purpose |
| --- | --- | --- |
| `--session-inactivity-secs` | `43200` | Cookie session inactivity expiry |
| `--cookie-secure` | `false` | Require HTTPS when sending the session cookie |

The SQLite adapter creates its parent directory, initializes the `http_sessions` table through the
store migration API, owns the expired-session cleanup task, and contributes an independent
`session_store` component to `/health`. Router authentication remains generic over
`tower_sessions::SessionStore`; no other session backend is currently wired or supported.

Run:

```bash
cargo test -p asset-http
```
