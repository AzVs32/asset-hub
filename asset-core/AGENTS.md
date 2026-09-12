# asset-core agent instructions

These instructions apply to `asset-core` and supplement the repository-level
[`AGENTS.md`](../AGENTS.md). This file is the module's architecture guide; no separate README is
required. Keep it synchronized with implementation changes. Distinguish implemented guarantees
from extension points and unverified behavior.

## Responsibility and dependency direction

`asset-core` owns framework-independent domain models, application services, and infrastructure
ports. Follow these boundaries:

- Keep Axum, SQLx, OpenDAL, filesystem paths, HTTP DTOs, and CLI parsing out of Core APIs.
- Put infrastructure requirements in the owning capability's `port` module. Implement adapters in
  `asset-infra`; let `asset-runtime` select adapters, assemble shared services, and own background work.
- Collaborate across aggregates through their public application services. Do not access another
  aggregate's private dependencies, constructors, lock registries, or storage-key helpers.
- Keep cross-aggregate orchestration in `workflow`. Do not give it a domain aggregate, storage port,
  or alternative persistence boundary.

## Module ownership

| Module | Owns |
| --- | --- |
| `directory` | Directory domain, persistence/query ports, and application services |
| `resource` | Resource domain, content and upload state, resource ports, and application services |
| `idempotency` | Durable request-idempotency types, repository port, and lease service |
| `storage` | Shared storage values and ports for bytes, directories, health checks, and scans |
| `workflow` | Application workflows coordinating aggregate services |
| `error` | Core-wide and aggregate-specific errors |
| `utils` | Crate-private shared implementation utilities |

- Keep `domain`, `port`, and `service` within their owning aggregate. Use capability-scoped imports
  such as `resource::service::ResourceService` and `directory::port::DirectoryStore`; do not introduce
  crate-wide domain, port, or service namespaces.
- Name Resource and Directory persistence interfaces `*Store` and rebuildable query interfaces
  `*ReadModel`. A `Projection` is the adapter-side combination of a read model and index writing.
- Group port definitions by cohesive lifecycle capability; do not split every trait into its own file.
- Put query inputs and result projections in the aggregate's `query` module. Keep ports limited to
  adapter contracts and durable rehydration values.

## Public API and assembly rules

| Consumer | API to expose |
| --- | --- |
| HTTP, CLI business operations, other Core workflows | Resource, Directory, Content, and Upload services |
| Infrastructure adapters | Capability-scoped ports, durable intent/rehydration values, `StorageKey` |
| Runtime, background work, trusted maintenance | Explicit recovery, upload-finalization, storage-maintenance, and Blob-readiness services |

- Do not expose repositories, Blob ports, locks, recovery, or maintenance operations through ordinary
  business services.
- Create aggregate management handles only through `DirectoryServices` or `ResourceServices`.
  Clone the assembled state so management and business services share ports, locks, and configuration.
  Do not reconstruct an independent service to obtain a narrower interface.
- Preserve `DirectoryServices` ownership of the shared Directory mutation lock and `ResourceServices`
  ownership of shared storage-key lock registries.
- Let Runtime create one configured `IdempotencyService` and inject it into Resource content and
  upload services. Do not construct another idempotency service inside an aggregate bundle.
- Keep process recovery ordering and background-task lifetimes in Runtime.

## Identity and storage rules

- Keep root-directory semantics on the `Directory` aggregate: `Directory::root()` assigns
  `DirectoryIdSlot::Slot0`; `Directory::is_root()` identifies it. `DirectoryId` exposes generic
  reserved-slot operations, not root-specific behavior.
- Keep stable Directory IDs on aggregates and paths in rebuildable query projections.
  `LocatedDirectory` pairs a `Directory` with its current `DirectoryPath`; `LocatedResource` pairs a
  `Resource` with its current Directory path. Do not persist full paths as aggregate state or
  duplicate identity in these projections.
- Keep `StorageKey` and `RESERVED_BLOB_STORAGE_PREFIX` in `storage`. Blob ports and adapters use
  those values directly, without depending on `resource::domain`.
- Keep conversion from a resolved Directory path plus Resource name to a visible Blob key inside
  Resource. Keep physical directory existence, creation, moves, and empty deletion behind
  `DirectoryStorage`.
- Preserve the current global idempotency-key scope: `idempotency_records.key` is the sole primary
  key across command types. Callers must use operation-unique keys. Adding operation namespaces
  requires an explicit compatibility change and migration.

## Content contracts

- Keep content reads streaming-only. Each `ContentReader` output chunk must be no larger than
  `MAX_CONTENT_READ_CHUNK_SIZE`; callers must not depend on exact chunk boundaries.
- Use half-open `[start, end)` byte ranges.
- Use the durable `UploadSession` state machine for both new resources and replacement content.
  Replacement uploads accept arbitrary binary bytes and share the offset and per-chunk checksum
  protocol with ordinary uploads.
- Preserve whole-content size and checksum validation, Resource revision checks, atomic publication,
  and durable replacement recovery before advancing the existing Resource revision.

## Recovery and consistency contracts

- Preserve durable intents, revision-checked updates, and recovery for Resource and Directory
  relocation. Do not describe them as one transaction across aggregates and filesystem storage.
- Treat an empty-directory precheck as advisory. The conditional repository deletion must check
  both emptiness and revision before authorizing the database deletion.
- Preserve per-key locking and compare-and-swap updates during storage reconciliation. Reconciliation
  converges over later scans; it is not a transactional snapshot.
- Keep physical file-identity checks inside Infra. Resource relocation and deletion recovery may
  finish an interrupted move with two links only when the recovery port confirms they identify the
  same physical file. Equal contents are insufficient. Fresh moves must reject occupied destinations.
- Treat archive manifests and ZIP streaming as best-effort views, not point-in-time snapshots.

Do not claim the following guarantees without implementing and verifying them first:

- Isolation between concurrent Resource relocation and Directory relocation.
- Isolation from external filesystem changes or arbitrary reconciliation event interleavings.
- Power-loss durability of filesystem directory entries.

## Validation

Apply the repository's AI-first test policy. Add or retain a test only when it prevents a specific,
non-obvious future mistake; keep the scenario at the layer that owns the guarantee.

For Core implementation changes, run:

```bash
cargo fmt --all -- --check
cargo clippy -p asset-core --all-targets -- -D warnings
cargo test -p asset-core
```

For shared-port or public-contract changes, also run workspace Clippy and tests and update affected
adapters, Runtime wiring, transport contracts, and documentation together. Documentation-only edits
require checking references and the diff; they do not require rerunning Rust tests.
