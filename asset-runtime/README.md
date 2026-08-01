# Asset Runtime

`asset-runtime` is the reusable application assembly layer. Each executable surface loads its own
configuration and creates one `AssetRuntime`; the runtime remains independent of HTTP routing, CLI
parsing, and presentation policy.

Construction has a deterministic order:

1. initialize concrete database, storage, index, and repository adapters through
   `AssetInfrastructure`;
2. load the built-in plugin catalog and read-only verify installed packages;
3. derive resource/directory kind and action registries;
4. compile Extism bindings and wrap built-in/Extism action executors;
5. compose `DirectoryService`, `ResourceService`, `UserService`, and `AuthorizationService` from
   Core ports;
6. resume pending upload finalizations;
7. start optional storage synchronization only when the application surface requests it.

Plugin package mutation is not part of runtime startup. Packages must be sealed explicitly through
`asset plugin --generate-lock` before loading. Business workflows, authorization, compensation,
and effect application remain in `asset-core`.

Run:

```bash
cargo test -p asset-runtime
```
