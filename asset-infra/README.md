# Asset Infrastructure

`asset-infra` contains concrete adapters for the host side of Asset Hub. It initializes the current
SQLite database, local OpenDAL blob storage, filesystem scanner/synchronizer, directory index,
identity/upload repositories, the Host-owned built-in capability catalog, Extism executor,
registries, and plugin package filesystem adapter.

Its business database schema does not contain HTTP authentication sessions. That surface-local
state is stored and migrated independently by `asset-http`; infrastructure database pools are not
exposed through `asset-runtime`.

It does not assemble Core services or decide application startup order. `AssetInfrastructure::new`
normalizes already-loaded configuration and initializes only the database, storage, index, and
repository adapters. `asset-runtime` consumes these ports and composes the plugin host and Core
services; after injection, the `AssetInfrastructure` aggregate itself is construction-only.

`LocalStorageSync` still implements the local filesystem watcher and event-to-reconciliation
driving adapter. `asset-runtime` starts it with `ResourceService` and owns its lifetime;
`AssetInfrastructure` no longer accepts a Core service or starts background work.

## Plugin package boundary

Built-in kinds and actions are Rust Host definitions with private typed handler bindings. They are
not parsed through `asset-plugin-api::PluginManifest` and never appear in the external package
catalog. Every filesystem package is an Extism/Wasm package; `runtime.type = "builtin"` is rejected.
## Resource kind and capability tree

The following is the complete Resource kind tree assembled by the Host and bundled plugins. The
annotation after each kind is its definition source. `core:resource` is the default kind.

```text
core:resource  [builtin:core.resource; default]
├─ core:image  [builtin:core.image]
├─ core:text  [builtin:core.text]
│  └─ azvs:markdown  [plugin:azvs.markdown]
├─ azvs:epub  [plugin:azvs.epub]
└─ core:video  [builtin:core.video]
```

Resource capabilities are singleton providers, not generic action names. For each capability, the
Host selects the provider declared on the nearest kind in the Resource lineage. A child provider
therefore replaces, rather than coexists with, its ancestor's provider. The currently supported
Resource capabilities are `thumbnail`, `text_read`, and `text_edit`.

| Kind | Resolved capability providers | Other available actions |
| --- | --- | --- |
| `core:resource` | `thumbnail` → `core.resource.thumbnail` | `core.resource.download` |
| `core:image` | `thumbnail` → `core.image.thumbnail` | inherits `core.resource.download` |
| `core:text` | `thumbnail` → `core.resource.thumbnail`; `text_read` → `core.text.read`; `text_edit` → `core.text.edit` | inherits `core.resource.download` |
| `azvs:markdown` | `thumbnail` → `core.resource.thumbnail`; `text_read` → `azvs.markdown.read`; `text_edit` → `azvs.markdown.edit` | inherits `core.resource.download` |
| `azvs:epub` | `thumbnail` → `azvs.epub.thumbnail` | inherits `core.resource.download`; `azvs.epub.render` |
| `core:video` | `thumbnail` → `core.resource.thumbnail` | inherits `core.resource.download` |

`core:text` detects `text/*`, common structured-text MIME types, and common text extensions.
More-specific descendants such as `azvs:markdown` still win when both definitions match.
`core:image` detects `image/*` and its declared image extensions; `core:video` detects `video/*` and
its declared video extensions; `azvs:epub` detects only the MIME types and extensions declared in
its plugin manifest. `core:document` is not a registered Resource kind.

The Host provides generic `core.resource.thumbnail` and `core.directory.thumbnail` actions.
The generic resource provider always returns a kind-neutral file thumbnail. The Host-owned
`core.image.thumbnail` action applies only to `core:image`, returns the authorized image content
URL, and provides the same `thumbnail` singleton capability as the generic provider.
The fixed generic artwork lives in `assets/thumbnails/resource.svg` and
`assets/thumbnails/directory.svg`. `include_str!` embeds both files into the Host binary at compile
time; deployment does not need to copy them as separate runtime files.
External actions retain their provider-owned IDs and may provide a Host-recognized capability for a
more specific kind. Resource actions recognize `thumbnail`, `text_read`, and `text_edit`; directory
actions recognize only `thumbnail`. Resource resolution filters content requirements and matchers
before selecting the nearest provider. Registry startup rejects unsupported capabilities, automatic
thumbnail-slot actions that do not provide `thumbnail`, and tied nearest providers.
At the package boundary, infrastructure explicitly converts external Manifest capabilities into
`asset-core` Action/Kind definitions. Extism handler names remain in private adapter bindings and
are not copied into Core models.

Extism memory, timeout, concurrency, serialized input/output, and Host ABI budgets are validated
in infrastructure policy. Runtime assembly derives the smaller runtime-independent Core resource
Action content policy from the same configured limits.

Interactive text editing has a separate Host policy because browser editing is not a plugin
execution budget. `[resource_edit].max_text_bytes` defaults to 4 MiB. Runtime passes that value to
Core, which uses it both when discovering `text_edit` providers and when validating streamed
replacement content. Resources above the limit therefore do not advertise `text_edit`.

Resource and Directory optimistic concurrency use persisted, monotonically increasing `revision`
values; timestamps remain display and ordering metadata. Directory writes compare the expected
revision atomically in SQLite, including effects applied after a directory Action returns.

Core Action and capability identifiers are validated domain values. Dynamic identifiers reject
empty, non-canonical, uppercase, or unsupported characters before reaching registries or executors;
Host-owned static declarations assert the same invariant at construction.

SQLite never deserializes persisted data directly into Resource, Directory, or User aggregates.
Repository rows first become unchecked snapshots and then pass through Core rehydration; persisted
Resource content, checksums, and storage keys likewise deserialize through their validating Core
constructors. Invalid JSON, path-like keys, aggregate timestamps, or other inconsistent rows are
reported as repository failures and no partially valid domain object is returned.

Persisted upload sessions are rehydrated through the Core aggregate state machine. SQLite
conditional updates only advance offsets while uploading, only enter finalization after all bytes
arrive, and only complete a checksummed finalization; inconsistent persisted rows are reported as
repository failures instead of being exposed as valid sessions.

Content replacement, whether requested by an editor or returned by a write Action, writes a durable
intent before moving the existing Blob. The intent stores the Resource ID, expected revision,
target/staging/backup keys, and replacement content metadata.
Runtime startup resolves every pending intent before upload recovery: a committed Resource keeps
the published Blob, while an unchanged Resource is restored from its backup. Internal artifacts are
removed only after the intent has reached a recoverable terminal state.

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
