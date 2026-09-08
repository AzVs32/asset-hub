# Asset Infrastructure

`asset-infra` contains the concrete SQLite, local OpenDAL storage, filesystem scanner/synchronizer,
directory-index, upload, and recovery-repository adapters used by Asset Hub.

`AssetInfrastructure::new` normalizes already-loaded configuration and initializes only those
adapters. It does not assemble Core services or start background tasks; `asset-runtime` owns both
composition and lifecycle.

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

Run:

```bash
cargo test -p asset-infra
```
