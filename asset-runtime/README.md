# Asset Runtime

`asset-runtime` is the reusable application assembly layer. Each executable surface assembles an
`asset-config` registry, loads the shared configuration document once, and passes the normalized
`[asset]` section into one `AssetRuntime`. Runtime remains independent of HTTP routing, CLI parsing,
configuration sources, and presentation policy.

Runtime owns `AssetConfig` and implements the `[asset]` `ConfigSection`; each executable registers
that type in its own composition root. `AssetConfig` composes the database and Blob
sub-configuration owned by `asset-infra` with Runtime's Resource editing and idempotency policies.
`asset-config` remains independent of all of these concrete settings.

Construction is deterministic:

1. initialize the SQLite, local storage, index, and repository adapters through
   `AssetInfrastructure`;
2. compose the Directory query, storage-import, and index services as one `DirectoryServices` bundle;
3. recover pending Directory relocations, Resource relocations, permanent Resource deletions, and
   content replacements before optional filesystem synchronization;
4. schedule pending upload finalizations through the Runtime-owned supervisor;
5. start optional storage synchronization only when the application surface requests it.

Runtime startup obtains Core's explicit recovery and upload-finalization management handles from
the assembled service bundles. Ordinary application surfaces continue to receive only the business
services. The health endpoint receives a narrow Blob readiness handle, while trusted local
maintenance and storage synchronization retain `StorageMaintenanceService`.

Runtime also creates the single configured `IdempotencyService` and injects it into consumers.
The SQLite record key is currently global across command types, so callers must not reuse an
idempotency key for a different operation. This contract does not add an operation namespace or
change existing persisted records.

Before creating a Resource relocation intent, Core rechecks the current revision and source path
under the Resource path locks, requires an existing source Blob, and rejects an occupied physical
destination. These rejected requests leave no new recovery intent. Interpreting a destination-only
state as an interrupted move is reserved for an intent that has already passed those checks.
Recovery also recognizes the two-hard-link state left by older Blob moves interrupted before
source removal. It finishes the move only when the adapter verifies physical file identity;
independent objects remain a conflict and the intent is retained for inspection.

`UploadSession` owns durable upload state transitions. Runtime owns the deduplicating finalization
supervisor and all spawned task lifetimes; application surfaces receive only the
`UploadFinalizationDispatcher` interface. `LocalStorageSync` remains an `asset-infra` driving
adapter, but Runtime starts it with `StorageMaintenanceService` and owns its lifetime.
The synchronization owner also owns its bounded startup verification futures (four at a time).
Dropping it cancels those workflows as well as the event loop; individual checksum errors or
unwinding panics do not stop the rest of the verification queue. Startup remains non-blocking with
respect to the HTTP listener. In-flight OS operations can finish after cancellation.

Run:

```bash
cargo test -p asset-runtime
```
