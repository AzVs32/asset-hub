# Asset Runtime

`asset-runtime` is the reusable application assembly layer. Each executable surface loads its own
configuration and creates one `AssetRuntime`; the runtime remains independent of HTTP routing, CLI
parsing, and presentation policy.

Construction is deterministic:

1. initialize the SQLite, local storage, index, and repository adapters through
   `AssetInfrastructure`;
2. compose the three Directory services as one `DirectoryServices` bundle;
3. recover pending Directory relocations, Resource relocations, and content replacements;
4. schedule pending upload finalizations through the Runtime-owned supervisor;
5. start optional storage synchronization only when the application surface requests it.

Runtime startup invokes Core business operations directly through their authorization-bound
services and does not retain extension-package lifecycle responsibilities.

`UploadSession` owns durable upload state transitions. Runtime owns the deduplicating finalization
supervisor and all spawned task lifetimes; application surfaces receive only the
`UploadFinalizationDispatcher` capability. `LocalStorageSync` remains an `asset-infra` driving
adapter, but Runtime starts it with `ResourceService` and owns its lifetime.

Run:

```bash
cargo test -p asset-runtime
```
