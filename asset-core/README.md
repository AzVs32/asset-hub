# asset-core

`asset-core` is Asset Hub's framework-independent domain and application kernel. It defines no
Axum, SQLx, OpenDAL, filesystem, or CLI types.

## Module boundaries

- `directory`: the Directory aggregate, its persistence/query ports, and directory application
  services.
- `resource`: the Resource aggregate, resource-content and upload state, resource ports, and
  resource application services.
- `idempotency`: durable request-idempotency domain types, repository port, and lease service.
- `storage`: ports for blob bytes, physical directories, health checks, and storage scans.
- `workflow`: cross-aggregate application workflows; it does not own a domain aggregate or a
  storage adapter port.
- `error`: Core-wide and aggregate-specific error types.
- `utils`: crate-private shared implementation utilities.

Within an aggregate boundary, `domain`, `port`, and `service` stay together. Consumers should use
the capability-scoped paths, for example `asset_core::resource::service::ResourceService` and
`asset_core::directory::port::DirectoryStore`, rather than a crate-wide domain, port, or service
namespace.

Resource and Directory persistence ports use the `Store` suffix. Rebuildable query interfaces use
the `ReadModel` suffix; a `Projection` is the adapter-side composition of a read model and its
index-writing capability. Port files remain grouped by cohesive lifecycle capability rather than
placing every trait in a separate file.

Service dependencies, path-lock registries, constructors, and storage-key helpers are private to
their owning aggregate. Other Core modules collaborate through the aggregate's public application
services, while adapters implement only its public ports.

Public types are separated by consumer. Aggregate `query` modules contain query inputs and result
projections; ports contain only adapter contracts and durable rehydration values. HTTP, CLI, and
cross-aggregate workflows receive the ordinary Resource, Content, Upload, and Directory services.
Process recovery, upload finalization, storage reconciliation, and Blob readiness have explicit
management services. Management handles are created only by `DirectoryServices` or
`ResourceServices` and clone the already-assembled service state, so they retain the same locks,
ports, and configuration rather than rebuilding services.

`storage::StorageKey` and `storage::RESERVED_BLOB_STORAGE_PREFIX` are shared storage values.
They are used directly by Blob ports and adapters, without a dependency on `resource::domain`.
Resource owns only the conversion from a resolved Directory path plus Resource name into a visible
Blob key. `DirectoryStorage` remains a physical-storage port because it abstracts directory
existence, creation, moves, and empty-directory deletion for the Directory service.

Content reads are streaming-only. Every chunk returned by `ContentReader` is bounded by
`MAX_CONTENT_READ_CHUNK_SIZE`, while callers must not depend on exact chunk boundaries. Range
reads use half-open `[start, end)` intervals.

The `Directory` aggregate owns root-directory semantics: `Directory::root()` assigns
`DirectoryIdSlot::Slot0`, and `Directory::is_root()` identifies that aggregate. `DirectoryId`
exposes only generic reserved-slot operations and does not define `root` or `is_root` behavior.

Runtime is the composition owner for `IdempotencyService`: it creates one service from the
idempotency repository and configured lease duration, then injects that shared capability into
the Resource content and upload services. The persisted key is globally scoped across command
types because `idempotency_records.key` is the sole primary key. Clients must use operation-unique
keys; a namespace migration is a separate compatibility change.

## Dependency direction

Core domain and application services depend only on their owning aggregate, other aggregate
application services, and capability-scoped Core ports. Adapters in `asset-infra` implement those
ports. `asset-runtime` selects implementations, creates shared capabilities, and manages
background work; HTTP and CLI call the resulting business or explicitly administrative interfaces.
`workflow` coordinates aggregate services where a workflow needs both aggregates, but does not
become an alternative persistence or storage boundary.

## Public API categories

| Category | Consumers | Contents |
| --- | --- | --- |
| Business API | HTTP, CLI, other Core workflows | Resource, Directory, Content, and Upload commands, queries, and content operations |
| Adapter API | `asset-infra` | Capability-scoped repository and storage ports, durable intent and rehydration values, `StorageKey` |
| Runtime-management API | Runtime, background work, trusted maintenance | Recovery, upload finalization, storage maintenance, Directory reconciliation, and Blob readiness services |

Ordinary business services do not expose repositories, Blob ports, locks, recovery, or maintenance
operations. Query inputs and projections belong to the aggregate `query` modules rather than a
repository port. Management handles clone the already-assembled aggregate state; callers never
reconstruct a second service merely to obtain a narrower interface.

Directory query results avoid duplicating aggregate identity: `LocatedDirectory` pairs a
`Directory` directly with its current `DirectoryPath`, while `LocatedResource` pairs a `Resource`
with its current Directory path. Stable Directory IDs remain on the owning aggregates, and full
paths remain rebuildable query projections rather than persisted aggregate state.

## Assembly and consistency boundaries

`DirectoryServices` owns the shared Directory mutation lock. `ResourceServices` owns the shared
storage-key lock registries and consumes the injected idempotency capability. Runtime creates the
idempotency service, receives recovery and management handles from those bundles, runs recovery in
its defined order, and owns upload-finalization and storage-synchronization task lifetimes.

Resource relocation and Directory relocation use durable intents plus version-checked updates and
recovery; they are not one transaction across aggregates or filesystem storage. Empty-directory
deletion is authoritative only at the conditional repository delete that checks emptiness and
revision together; a preliminary check is advisory. Storage reconciliation uses per-key locks and
compare-and-swap updates, then converges on later scans. Archive manifests and ZIP streaming are
best-effort views, not point-in-time snapshots. Cross-aggregate races between Resource relocation
and Directory relocation, and filesystem-event interleavings during reconciliation, remain
explicitly unverified concurrency guarantees.
