# Asset Infrastructure

`asset-infra` contains the concrete SQLite, local OpenDAL storage, filesystem scanner/synchronizer,
directory-index, upload, and recovery-repository adapters used by Asset Hub.

`asset-infra` owns the database and Blob configuration types consumed by its adapters.
`asset-runtime` includes those types in its `[asset]` configuration and passes `DatabaseConfig` and
`BlobConfig` directly to `AssetInfrastructure::new`. Both public construction boundaries validate
the configuration before initialization. Infrastructure initializes only adapters; it does not load
configuration sources, assemble Core services, or start background tasks. Runtime owns service
composition and lifecycle.

`DatabaseConfig` and `BlobConfig` are Serde models embedded in `asset-runtime::AssetConfig`.
They own their backend-specific defaults, normalization, and validation. Neither type is registered
as a separate configuration section because both belong to the `[asset]` subtree.

Adapters implement capability-scoped Core ports. Blob adapters and scanners use
`storage::StorageKey` directly; they do not depend on the Resource domain merely to validate a
key. The SQLite idempotency repository is supplied to Runtime, which configures and injects the
shared idempotency service instead of any aggregate bundle creating it.

SQLite keeps Resource and Directory persistence boundaries separate despite sharing a connection
pool. Directory relocation, Resource content replacement, and permanent Resource deletion use
durable intents so Runtime recovery can converge the database and local filesystem after
interruption. A deletion intent remains after the Resource row commits until its internally staged
Blob is removed. `LocalStorageSync` remains the filesystem event-to-reconciliation adapter; Runtime
owns its guard and task lifetime.

The storage scanner reports physical directories; Core imports those observed paths into Directory
aggregates.

The SQLite upload-session table persists upload state and idempotency linkage.

Local Blob operations preserve storage-key spelling, including leading and trailing spaces, for
existence checks as well as reads, moves and deletes. Linux, Android and Apple targets use an
atomic no-replace rename for Blob moves; errors are propagated without an overwrite fallback.
Other targets retain the hard-link/unlink protocol. Recovery recognizes its interrupted dual-link
state by physical file identity, never by matching content. Symlinks and independent files are
rejected without deleting either path. These filesystem operations run on blocking workers.
Only the Linux implementation has been validated in this development environment.

Startup checksum verification runs at most four checks concurrently, alongside the ordinary serial
event/periodic reconciliation loop. Checks are bounded futures owned by the synchronization task,
not detached per-file tasks. Individual errors and unwinding panics are logged without stopping the
remaining queue; periodic reconciliation can retry failed verification. Dropping `LocalStorageSync`
cancels the owner and drops its checks when cancellation is polled. Already-running OS I/O may
finish, but the cancelled verification futures do not continue their workflow. Initial scanning and
verification remain in the background and do not delay HTTP listener startup.

Run:

```bash
cargo test -p asset-infra
```
