# Asset Runtime

`asset-runtime` is the reusable application assembly layer. Each executable surface loads its own
configuration and creates one `AssetRuntime`; the runtime remains independent of HTTP routing, CLI
parsing, and presentation policy.

Construction is deterministic:

1. initialize the SQLite, local storage, index, and repository adapters through
   `AssetInfrastructure`;
2. construct the Host-owned static Kind and Action catalog and inject its registries and handlers
   into Core services;
3. compose the three Directory services as one `DirectoryServices` bundle;
4. recover pending Directory relocations, Resource relocations, and content replacements;
5. schedule pending upload finalizations through the Runtime-owned supervisor;
6. start optional storage synchronization only when the application surface requests it.

Runtime startup does not discover package directories, validate Manifests, compile Wasm, construct
a plugin Host, or retain browser asset snapshots. The static Kind and Action definitions remain
temporarily because later removal steps still consume those Core contracts.

`UploadSession` owns durable upload state transitions. Runtime owns the deduplicating finalization
supervisor and all spawned task lifetimes; application surfaces receive only the
`UploadFinalizationDispatcher` capability. `LocalStorageSync` remains an `asset-infra` driving
adapter, but Runtime starts it with `ResourceService` and owns its lifetime.

Run:

```bash
cargo test -p asset-runtime
```
