# Asset HTTP

`asset-http` is the Axum transport and the composition root for HTTP-only policy. It owns routing,
cookie authentication, the OpenAPI JSON contract, request limits, CORS, and the
authentication-session store.

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

Action declarations expose the views and effects they may return through `output.views` and
`output.effects`. The built-in Resource and Directory delete entries are ordinary write Actions
that declare only the `delete` effect, carry destructive confirmation metadata, and return no View.
Clients invoke them through the same Resource or Directory action endpoint used by other Actions.
Core applies the effect through the existing authorized resource soft-delete or empty-directory
delete use case. The dedicated `DELETE /resources/{id}` and `DELETE /directories/{id}` endpoints
remain available as direct HTTP use cases.

Directory downloads use ordinary ZIP entries for directories and resources up to 4 GiB. ZIP64 is
enabled only for an individual resource that exceeds the ZIP32 size limit, keeping ordinary
downloads compatible with desktop archive tools without removing large-resource support.

## OpenAPI contract

The generated OpenAPI document is exposed directly as JSON at `/api-docs/openapi.json`. Asset Hub
does not bundle or serve Swagger UI. The JSON endpoint remains public so `asset-web` can regenerate
its checked-in transport declarations with `npm run generate:api`; browsers do not request this
document during normal application use.

## Plugin Web asset policy

Verified plugin Web assets are public immutable snapshots served from `/plugins/<plugin-id>/...`.
Their Content Security Policy permits scripts, styles, images, media, fonts, and nested frames only
from the Asset Hub origin (with data URLs additionally allowed for visual assets). Network
connections and objects remain disabled. Same-origin nested frames are required when a Directory
workspace mounts a Host-resolved Resource provider, such as the Games workspace displaying a
README through the Resource Text reader; both frames still run in the Host's script-only iframe
sandbox.

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
