# Asset Infrastructure

`asset-infra` contains concrete adapters for the host side of Asset Hub. It initializes the current
SQLite database, local OpenDAL blob storage, filesystem scanner/synchronizer, directory index,
identity/audit/upload repositories, the Host-owned built-in capability catalog, Extism executor,
registries, and plugin package filesystem adapter.

It does not assemble Core services or decide application startup order. `AssetInfrastructure::new`
normalizes already-loaded configuration and initializes only the database, storage, index, and
repository adapters. `asset-runtime` consumes these ports and composes the plugin host and Core
services.

## Plugin package boundary

Built-in kinds and actions are Rust Host definitions with private typed handler bindings. They are
not parsed through `asset-plugin-api::PluginManifest` and never appear in the external package
catalog. Every filesystem package is an Extism/Wasm package; `runtime.type = "builtin"` is rejected.
The Host provides generic `core.resource.thumbnail` and `core.directory.thumbnail` actions.
The generic resource provider always returns a kind-neutral file thumbnail. The Host-owned
`core.image.thumbnail` action applies only to `core:image`, returns the authorized image content
URL, and provides the same `thumbnail` singleton capability as the generic provider.
The fixed generic artwork lives in `assets/thumbnails/resource.svg` and
`assets/thumbnails/directory.svg`. `include_str!` embeds both files into the Host binary at compile
time; deployment does not need to copy them as separate runtime files.
External actions retain their provider-owned IDs and may provide the Host-recognized `thumbnail`
capability for a more specific kind. Resource and directory registries scope that capability
independently. Resource resolution filters content requirements and matchers before selecting the
nearest provider. Registry startup rejects unsupported capabilities, automatic thumbnail-slot
actions that do not provide `thumbnail`, and tied nearest providers.
At the package boundary, infrastructure explicitly converts external Manifest capabilities into
`asset-core` Action/Kind definitions. Extism handler names remain in private adapter bindings and
are not copied into Core models.

Extism memory, timeout, concurrency, serialized input/output, and Host ABI budgets are validated
in infrastructure policy. Runtime assembly derives the smaller runtime-independent Core resource
Action content policy from the same configured limits.

`plugin_package` is the single public host boundary for package discovery and verification. Both
the CLI and runtime use it. Its two workflows are intentionally separate:

- `generate_plugin_manifest_lock` validates an unsealed package and atomically creates a missing
  `manifest.lock.json`; it never replaces an existing lock.
- `load_verified_plugin_package` and `PluginCatalog::load` are read-only, require an existing lock,
  verify every declared digest and file, enforce the package layout and size limits, and retain
  verified Wasm/Web byte snapshots.

The fixed limits are 1 MiB for `manifest.json`, 4 MiB for `manifest.lock.json`, 64 MiB for
`plugin.wasm`, and 64 MiB in aggregate for Web assets. Package trees cannot contain symbolic links
or special files.

Run:

```bash
cargo test -p asset-infra
```
