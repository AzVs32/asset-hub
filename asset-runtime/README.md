# Asset Runtime

`asset-runtime` is the reusable application assembly layer. Each executable surface loads its own
configuration and creates one `AssetRuntime`; the runtime remains independent of HTTP routing, CLI
parsing, and presentation policy.

Construction is deterministic:

1. initialize the SQLite, local storage, index, and repository adapters through
   `AssetInfrastructure`;
2. compose the Directory query, storage-import, and index services as one `DirectoryServices` bundle;
3. recover pending Directory relocations, Resource relocations, permanent Resource deletions, and
   content replacements before optional filesystem synchronization;
4. schedule pending upload finalizations through the Runtime-owned supervisor;
5. start optional storage synchronization only when the application surface requests it.

Runtime startup invokes Core business operations directly through their Core service boundaries.

Before creating a Resource relocation intent, Core rechecks the current revision and source path
under the Resource path locks, requires an existing source Blob, and rejects an occupied physical
destination. These rejected requests leave no new recovery intent. Interpreting a destination-only
state as an interrupted move is reserved for an intent that has already passed those checks.

`UploadSession` owns durable upload state transitions. Runtime owns the deduplicating finalization
supervisor and all spawned task lifetimes; application surfaces receive only the
`UploadFinalizationDispatcher` interface. `LocalStorageSync` remains an `asset-infra` driving
adapter, but Runtime starts it with `ResourceService` and owns its lifetime.

Run:

```bash
cargo test -p asset-runtime
```
