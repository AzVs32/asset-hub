# Asset Infrastructure

`asset-infra` contains the concrete SQLite, local OpenDAL storage, filesystem scanner/synchronizer,
directory-index, identity, upload, and recovery-repository adapters used by Asset Hub.

`AssetInfrastructure::new` normalizes already-loaded configuration and initializes only those
adapters. It does not assemble Core services or start background tasks; `asset-runtime` owns both
composition and lifecycle. HTTP authentication sessions remain an `asset-http` concern and do not
share the business database pool.

SQLite keeps Resource and Directory persistence boundaries separate despite sharing a connection
pool. Directory relocation and Resource content replacement use durable intents so Runtime recovery
can converge the database and local filesystem after interruption. `LocalStorageSync` remains the
filesystem event-to-reconciliation adapter; Runtime owns its guard and task lifetime.

The SQLite upload-session table persists upload state and idempotency linkage, but no user owner.

Run:

```bash
cargo test -p asset-infra
```
