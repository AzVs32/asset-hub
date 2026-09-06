# Asset HTTP

`asset-http` is the Axum transport and the composition root for HTTP-only policy. It owns routing,
the OpenAPI JSON contract, request limits, and CORS.

Business handlers receive Core application services. Upload completion receives the narrow
`UploadFinalizationDispatcher` interface; HTTP does not depend on the concrete Runtime
scheduler or supervisor.

Upload sessions are not owned by a user in Core or the business database. Upload routes invoke
`UploadService` directly; their response and resumable-client contracts are unchanged.

Resource, Directory, Content, and archive handlers invoke their direct Core services with global
directory paths: the nil UUID root is represented by an empty path. HTTP does not project paths into
workspaces or pass user or authorization context to handlers. Router construction receives one
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
does not bundle or serve Swagger UI. `asset-web` regenerates its checked-in transport declarations
with `npm run generate:api`; browsers do not request this document during normal application use.

Run:

```bash
cargo test -p asset-http
```
