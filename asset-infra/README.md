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
annotation after each kind is its typed definition origin. `core:resource` is the default kind.

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
| `core:resource` | `thumbnail` → `core.resource.thumbnail` | `core.resource.download`; effect-only action `core.resource.delete` |
| `core:image` | `thumbnail` → `core.image.thumbnail` | inherits download and delete |
| `core:text` | `thumbnail` → `core.resource.thumbnail`; `text_read` → `core.text.read`; `text_edit` → `core.text.edit` | inherits download and delete |
| `azvs:markdown` | `thumbnail` → `core.resource.thumbnail`; `text_read` → `azvs.markdown.read`; `text_edit` → `azvs.markdown.edit` | inherits download and delete |
| `azvs:epub` | `thumbnail` → `azvs.epub.thumbnail` | inherits download and delete; `azvs.epub.render` |
| `core:video` | `thumbnail` → `core.resource.thumbnail` | inherits download and delete |

`core:text` detects `text/*`, common structured-text MIME types, and common text extensions.
More-specific descendants such as `azvs:markdown` still win when both definitions match.
`core:image` detects `image/*` and its declared image extensions; `core:video` detects `video/*` and
its declared video extensions; `azvs:epub` detects only the MIME types and extensions declared in
its plugin manifest. `core:document` is not a registered Resource kind.

The Host provides generic `core.resource.thumbnail` and `core.directory.thumbnail` actions.
It also declares `core.resource.delete` and `core.directory.delete` as ordinary write Actions in
the same discovery catalogs. Their built-in handlers return no View and request one `delete`
effect; Core applies it through the secured resource soft-delete or empty-directory-delete use
case. External plugins may declare and return the same effect only when their Manifest requests
`resource.delete` or `directory.delete`, the corresponding `[plugin.grants]` switch is enabled,
and the current user is authorized to delete that aggregate. Delete cannot be combined with a
different effect in one action output.
The generic resource provider always returns a kind-neutral file thumbnail. The Host-owned
`core.image.thumbnail` action applies only to `core:image`, returns the authorized image content
URL, and provides the same `thumbnail` singleton capability as the generic provider.
The fixed generic artwork lives in `assets/thumbnails/resource.svg` and
`assets/thumbnails/directory.svg`. `include_str!` embeds both files into the Host binary at compile
time; deployment does not need to copy them as separate runtime files.
External actions retain their provider-owned IDs and may provide a Host-recognized capability for a
more specific kind. Resource actions recognize `thumbnail`, `text_read`, and `text_edit`; Directory
actions recognize `thumbnail` and `workspace`. A Directory `workspace` provider is read-only,
effect-free, supports `plugin_frame`, and pairs capability `workspace` with the
exclusive `directory_workspace` location. Resource resolution filters content requirements and matchers
before selecting the nearest provider. Registry startup rejects unsupported capabilities, automatic
thumbnail-slot actions that do not provide `thumbnail`, and tied nearest providers.
Ordinary Resource and Directory actions declared on an ancestor kind are inherited by its
descendants. Directory action discovery and execution use the same lineage-resolved set, so an
inherited action does not need to repeat every descendant kind in its declaration.
When a plugin Resource capability provider omits its Manifest label, catalog assembly inherits the
normalized label from the nearest ancestor provider for that capability; an explicit label remains
an override, and a missing ancestor is a startup error. The built-in `text_read` and `text_edit`
labels are `Read` and `Edit`. An external `text_edit` provider must declare write access and the
specific `resource.content.replace` permission; generic Resource write permissions are not
accepted.
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
Resource and Directory Kind IDs use two or more lowercase colon-separated segments; the segments
are identity only, while inheritance is declared explicitly through `parent`. Resource and
Directory Action IDs use lowercase dot-separated provider names such as `azvs.markdown.read`.
Runtime constructs all four registries
as one validated capability-catalog unit, rejects duplicate IDs and invalid scopes before serving,
and reports ambiguous content-kind detection instead of selecting by registration order.
Definition origins also carry validated lowercase dot-separated owner IDs rather than unchecked
strings.

Directory Kind declarations may restrict direct placement with `allowed_parent_kinds`. Catalog
assembly verifies every referenced parent Kind, and Core enforces the effective nearest constraint
on create, move, and Kind changes. A parent descendant satisfies an allowed ancestor. No Resource
filename, role, or required-file policy is normalized by the Host.

Directory Actions choose resource exposure with `requires.resources = none | metadata | content`.
Metadata mode exposes paged Resource identity, Kind, revision, and content metadata. Content mode
also creates call-scoped handles and reuses the standard content open/read/close Host ABI; it
requires both Resource read permissions in addition to Directory resource listing. Leases are
destroyed when the Directory Action invocation ends, so plugins receive neither persistent handles
nor storage paths. Interpretation of special files and Resource Kinds remains plugin-owned.

Plugin loading intentionally remains atomic and fail-fast. Package verification, cross-plugin
capability conflict checks, Wasm compilation, executor bindings, and verified Web snapshots all
describe one runtime generation. Silently skipping a package in only one phase would expose a
partially assembled catalog, so a broken or conflicting package prevents that generation from
starting and must be fixed or removed explicitly.

SQLite never deserializes persisted data directly into Resource, Directory, or User aggregates.
Repository queries first map columns into adapter-owned row structures, parse their database
representations into Core values, and then call the aggregate rehydration methods. Persisted Resource
content, checksums, and storage keys likewise deserialize through their validating Core constructors.
Invalid JSON, path-like keys, aggregate timestamps, or other inconsistent rows are reported as
repository failures and no partially valid domain object is returned.

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

`plugin_package` is the single public host boundary for package installation, uninstallation,
discovery, and verification. Both the CLI and runtime use it. Its workflows are intentionally
separate:

- `install_plugin_package` accepts an arbitrarily named local source directory, snapshots its
  validated Manifest/Wasm/Web bytes into same-filesystem staging, generates a fresh lock, verifies
  the staged canonical package, and only then installs or replaces it. It never mutates the source.
- `uninstall_plugin_package` moves one ID-addressed canonical package out of the discovery root
  before deleting it.
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
