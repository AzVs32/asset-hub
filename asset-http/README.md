# Asset HTTP

`asset-http` is the Axum transport and the composition root for HTTP-only policy. It owns routing,
cookie authentication, the OpenAPI JSON contract, request limits, CORS, and the
authentication-session store.

Business handlers receive Core application services. Upload completion receives the narrow
`UploadFinalizationDispatcher` interface; HTTP does not depend on the concrete Runtime
scheduler or supervisor.

Resource and Directory handlers live in separate modules. HTTP state exposes separately secured
Resource and Directory surfaces, plus `AssetWorkflowService` only for archive projections whose
result spans both aggregates. Router construction receives one
`HttpComposition` bundle: `ResourceHttpServices`, `DirectoryHttpServices`, the cross-aggregate
workflow service, and the narrow health-only Blob readiness interface. These bundles organize
transport dependencies only; they do not add business workflows or expose repositories, storage,
recovery, or reconciliation operations to handlers.

When explicit CORS origins are configured, browser preflight requests may send the documented
write precondition and upload headers, including `Idempotency-Key`.

Resource and Directory contracts deliberately use the same shape where their semantics overlap.
Both expose stable UUIDs and monotonically increasing `revision` values. Directory creation accepts a stable `parent_id`;
`GET`, `PATCH`, and `DELETE /directories/{id}` address the aggregate by UUID. Mutating Resource and
Directory requests require `expected_revision` (streaming content replacement uses `If-Match`) and
return a coded revision conflict when another writer has advanced the aggregate. Path strings
remain navigation and display data, not Directory identity.

Resource responses expose one authoritative `state` object derived by Core. It contains the
lifecycle state, content state, and effective single-value state. HTTP does not also expose a
second content-verification status; clients must consume `state` instead of reconstructing
precedence from independent transport fields.

Resource and Directory deletion uses the direct `DELETE /resources/{id}` and
`DELETE /directories/{id}` endpoints.

Directory downloads use ordinary ZIP entries for directories and resources up to 4 GiB. ZIP64 is
enabled only for an individual resource that exceeds the ZIP32 size limit, keeping ordinary
downloads compatible with desktop archive tools without removing large-resource support.

## OpenAPI contract

The generated OpenAPI document is exposed directly as JSON at `/api-docs/openapi.json`. Asset Hub
does not bundle or serve Swagger UI. The JSON endpoint remains public so `asset-web` can regenerate
its checked-in transport declarations with `npm run generate:api`; browsers do not request this
document during normal application use.

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
