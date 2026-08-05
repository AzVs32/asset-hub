# Asset Runtime

`asset-runtime` is the reusable application assembly layer. Each executable surface loads its own
configuration and creates one `AssetRuntime`; the runtime remains independent of HTTP routing, CLI
parsing, and presentation policy.

Construction has a deterministic order:

1. initialize concrete database, storage, index, and repository adapters through
   `AssetInfrastructure`;
2. construct the Host-owned built-in capability catalog and read-only verify external Extism
   packages;
3. derive resource/directory kind and action registries;
4. compile private Extism handler bindings and combine them with typed built-in handler bindings;
5. derive the Core Action-content and interactive text-edit policies from their independent Host
   configuration values;
6. compose `DirectoryService`, `ResourceService`, `UserService`, and `AuthorizationService` from
   Core ports;
7. recover pending Resource content replacements;
8. resume pending upload finalizations;
9. start optional storage synchronization only when the application surface requests it.

Plugin package mutation is not part of runtime startup. Packages must be sealed explicitly through
`asset plugin --generate-lock` before loading. Business workflows, authorization, compensation,
and effect application remain in `asset-core`.

The runtime owns the verified browser-asset snapshot exposed to application surfaces. Filesystem
paths and loaded bytes are Host runtime data and are intentionally absent from `asset-plugin-api`.

Run:

```bash
cargo test -p asset-runtime
```
