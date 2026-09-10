# Asset HTTP

`asset-http` is the Axum transport and the composition root for HTTP-only policy. It owns routing,
the OpenAPI JSON contract, request limits, and CORS.

Business handlers receive Core application services. Upload completion receives the narrow
`UploadFinalizationDispatcher` interface; HTTP does not depend on the concrete Runtime
scheduler or supervisor.

Upload routes invoke `UploadService` directly.

Resource, Directory, Content, and archive handlers invoke their direct Core services with global
directory paths: the root Directory's canonical path is empty. Router construction receives one
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

A Directory PATCH containing only a matching `expected_revision` leaves the aggregate unchanged,
including the root Directory. Root rename/move requests remain conflicts, and an outdated revision
still returns `concurrency.revision_conflict`.

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

## Directory ZIP execution and limits

The HTTP executable owns one `ArchiveRuntime` and passes its `ArchiveDownloads` handle through
`HttpComposition`. Embedders must retain that owner and call `shutdown().await` on exit; dropping
the owner also cancels outstanding work. Compression, SHA-256 verification, temporary-file
creation/writes, ZIP finalization, and rewind run on owned blocking workers. Content is obtained
only through Core's manifest and content services. A bounded channel carries at most four 64 KiB
content chunks; the producer and consumer each retain at most their current chunk (plus the
storage adapter's source buffer). File bytes are never collected into a complete in-memory file.
Manifest and ZIP entry metadata still scale with the number of entries.

HTTP CLI options (not TOML):

| Option | Default | Meaning |
| --- | --- | --- |
| `--archive-max-concurrent` | `2` | Maximum simultaneous archive requests, including manifest enumeration, generation and completed ZIPs still being downloaded |
| `--archive-max-bytes` | `68719476736` (64 GiB) | Per-request limit for both total uncompressed Resource bytes and the generated ZIP file length, including headers and central-directory metadata |

Both values must be nonzero. There is no waiting queue: exhausted capacity or shutdown returns
`503` with `archive.unavailable` and `retryable: true`. Resource sizes are checked before generation;
ZIP output is bounded at every write, including header rewrites and finalization. Exceeding either
limit returns `413`, without sending a partial ZIP. With defaults, these requests retain at most
128 GiB of ZIP file data per process; this is a ceiling, not a disk-space reservation. Insufficient
OS temporary space or other storage failures still return `500`. Operators can lower both limits
for their available temporary filesystem, for example:

```bash
asset-http --archive-max-concurrent 1 --archive-max-bytes 8589934592
```

The complete archive is generated before the `200` response, preserving `Content-Length`,
`Content-Disposition`, `application/zip` and `Cache-Control: no-store`. The temporary file is
anonymous and removed on close, including process exit. Its concurrency permit remains held
until the response body is consumed or dropped, so slow downloads cannot accumulate extra ZIPs.
Request cancellation drops the producer and cancels its worker; generation failures close the
file and release capacity. On SIGINT/SIGTERM the executable stops accepting HTTP requests and
cancels archive generation/downloads, allows HTTP connections up to 30 seconds to drain, then
joins all archive workers. Cancellation wakes a worker waiting on an idle input channel and is
checked at each bounded write. An already-running OS filesystem call cannot be forcibly
interrupted; worker joining may wait for that syscall to return. No detached compression task is
used as a timeout workaround. Other Runtime background services retain their existing lifecycle.

## Concurrent changes during ZIP generation

A directory download is a best-effort enumeration, **not a transactional point-in-time snapshot**.
Core records each entry's archive path, stable Resource ID, size and available SHA-256 while
walking the tree. The worker checks actual byte counts and, when a checksum is available, hashes
the bytes it writes. A size/checksum mismatch returns `409`; callers should refresh and retry.
Resources whose checksum is still unavailable have size verification only, so equal-size changes
cannot be detected for those entries.

Archive names/paths stay as captured by the manifest. Reads resolve each Resource by stable ID,
so a resource or ancestor directory moved before its read may still download under its captured
archive path. Missing content returns `404` when Core reports it absent; an I/O error during an
already-open read returns `500`. Deletion or modification after an entry has been read does not
invalidate those captured bytes. Additions and changes during tree enumeration can appear or be
omitted; successful verification does not imply all entries existed together at one instant.
The existing root name (`asset-hub`), empty directories, Unicode filenames, directory layout and
per-entry ZIP32/ZIP64 selection are preserved. Limits can be raised to allow larger archives.
