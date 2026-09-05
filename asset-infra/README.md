# Asset Infrastructure

`asset-infra` contains the concrete SQLite, local OpenDAL storage, filesystem scanner/synchronizer,
directory-index, identity, upload, and recovery-repository adapters used by Asset Hub.

`AssetInfrastructure::new` normalizes already-loaded configuration and initializes only those
adapters. It does not assemble Core services or start background tasks; `asset-runtime` owns both
composition and lifecycle. HTTP authentication sessions remain an `asset-http` concern and do not
share the business database pool.

The Host-owned static catalog currently defines the built-in Resource and Directory Kinds. It has
no plugin catalog dependency, package installation or verification support, Wasm executor, Host
ABI, permissions, execution budget, browser-frame asset handling, or Action executors.

SQLite keeps Resource and Directory persistence boundaries separate despite sharing a connection
pool. Directory relocation and Resource content replacement use durable intents so Runtime recovery
can converge the database and local filesystem after interruption. `LocalStorageSync` remains the
filesystem event-to-reconciliation adapter; Runtime owns its guard and task lifetime.

Run:

```bash
cargo test -p asset-infra
```
