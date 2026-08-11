# Asset Runtime

`asset-runtime` is the reusable application assembly layer. Each executable surface loads its own
configuration and creates one `AssetRuntime`; the runtime remains independent of HTTP routing, CLI
parsing, and presentation policy.

Concrete business database pools remain private to `asset-infra`. HTTP authentication sessions are
initialized and owned by `asset-http` through a separate store, pool, schema, and lifecycle; they are
not part of runtime assembly.

Construction has a deterministic order:

1. initialize concrete database, storage, index, and repository adapters through
   `AssetInfrastructure`;
2. construct the Host-owned built-in capability catalog and read-only verify external Extism
   packages;
3. derive resource/directory kind and action registries;
4. compile private Extism handler bindings and combine them with typed built-in handler bindings;
5. derive the Core Action-content and interactive text-edit policies from their independent Host
   configuration values;
6. compose one shared `DirectoryService`, then inject clones of that same service into
   `ResourceService`, `UserService`, and `AuthorizationService` so directory mutation locking has
   one process-local ownership boundary;
7. recover pending Resource content replacements;
8. read pending upload finalization IDs from Core and schedule them through the Runtime-owned
   finalization supervisor;
9. start optional storage synchronization only when the application surface requests it.

`AssetRuntime::new` is the composition boundary. `AssetInfrastructure`, `PluginCatalog`, concrete
kind/action registries, and concrete action executors are construction locals. Their required
ports and handler ownership are retained by the composed Core services; the Runtime does not keep
duplicate concrete `Arc`s or expose registry getters. Resource and Directory kind definitions are
queried through `ResourceService` and its shared `DirectoryService`.

The Runtime retains only the application services, the frozen Plugin Web asset snapshot, the
private upload-finalization supervisor, the effective settings needed to start local storage sync,
and the sync guard after startup. The caller continues to own its loaded configuration; the full
`AssetInfrastructure` aggregate is released when construction finishes.

Plugin package mutation is not part of runtime startup. Packages must be sealed explicitly through
`asset plugin --seal <plugin-id>` before loading. Business workflows, authorization, compensation,
and effect application remain in `asset-core`.

`UploadSession` owns its state-transition invariants. Creation and persistence rehydration reject
inconsistent offsets, checksums, failure reasons, timestamps, and terminal states. Core atomically
advances a requested upload to `Finalizing` and executes one finalization use case; Runtime owns the
deduplicating supervisor and all spawned task lifetimes. Application surfaces receive only the
`UploadFinalizationDispatcher` capability, not the concrete scheduler, queue, or supervisor. HTTP
submits a dispatch request after Core accepts the transition.

The local filesystem watcher and event interpretation remain an `asset-infra` driving adapter for
now. Runtime, rather than `AssetInfrastructure`, connects that adapter to `ResourceService` and owns
its guard. Splitting the watcher into a service-independent event source remains a possible future
refinement; it is not current runtime wiring.

The runtime owns the verified browser-asset snapshot exposed to application surfaces. Filesystem
paths and loaded bytes are Host runtime data and are intentionally absent from `asset-plugin-api`.

Run:

```bash
cargo test -p asset-runtime
```
