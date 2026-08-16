# Asset Plugin API

`asset-plugin-api` is the public Rust contract for external Asset Hub plugins.
It provides the shared types used to describe an Extism/Wasm package, declare
its capabilities, exchange action data with the host, and access supported Host
functions from Wasm. Host-owned built-in providers and their handler bindings
are intentionally outside this crate. The SDK also excludes Host-normalized
Action/Kind definitions, execution configuration, and loaded package snapshots.

Use this crate when building an Asset Hub plugin runtime or another tool that
needs to read and validate the plugin contract. Applications that only consume
the Asset Hub HTTP API do not need it.

## What It Provides

- Manifest models and validation.
- Resource and directory action request and response types.
- Singleton resource and directory capabilities with kind-specific providers.
- Structured views, effects, failures, and diagnostics.
- Versioned content and directory Host function definitions.
- Optional Extism guest helpers for Wasm plugins.

## Supported Versions

Asset Hub versions the Rust library and its serialized contracts separately:

| Surface | Current value | Purpose |
| --- | --- | --- |
| Rust crate | `0.4.0` | Rust source API |
| Manifest | `3` | external Extism/Wasm `manifest.json` document format |
| Plugin API | `asset-hub.plugin-api@5` | Action JSON, Host functions, and Plugin Frame Web SDK |

Plugins must declare both `manifest_version` and `runtime.plugin_api`. The host
rejects unsupported contract versions instead of attempting to interpret them.
The only supported runtime discriminator is `"type": "extism"`; `builtin` is
not a plugin runtime value and is rejected during deserialization.

Plugin API `@5` is intentionally incompatible with `@4`. It adds the `resource.create`
permission, the Directory `create_tree` effect, descendant-scoped Directory resource queries, and
the corresponding `@5` Resource and Directory frame channels. Plugins must be rebuilt against the
`0.4` Rust/Web SDK and redeclare `runtime.plugin_api` before installation.

## Getting Started

Add `asset-plugin-api` to the plugin runtime's `Cargo.toml`. Enable
`extism-guest` when the Wasm guest needs the provided Host function helpers:

```toml
[dependencies]
asset-plugin-api = { path = "<path-to-asset-plugin-api>", features = ["extism-guest"] }
```

Create a `manifest.json` that identifies the plugin and declares its runtime,
capabilities, and permissions:

```json
{
  "manifest_version": 3,
  "plugin": {
    "id": "example.plugin",
    "name": "Example Plugin",
    "version": "0.1.0",
    "publisher": "example"
  },
  "runtime": {
    "type": "extism",
    "plugin_api": "asset-hub.plugin-api@5"
  },
  "capabilities": {
    "resource_kinds": [
      {
        "kind": "example:document",
        "label": "Document",
        "supports_content": true
      }
    ],
    "resource_actions": [
      {
        "id": "example.plugin.inspect",
        "label": "Inspect",
        "description": "Inspect this document",
        "handler": "inspect",
        "applies_to": {
          "kinds": ["example:document"]
        },
        "output": { "views": ["json"] }
      }
    ]
  },
  "permissions": {
    "allow": ["resource.read"]
  }
}
```

An Extism plugin package uses `plugin.wasm` as its runtime entry. It may also
provide an `index.html` Web interface. The action handler exports referenced by
the Manifest exchange JSON values defined by this crate.

Build a local package directory containing `manifest.json`, `plugin.wasm`, and any Web assets, then
install that directory. Its directory name is arbitrary because the Manifest owns plugin identity:

```bash
asset plugin --install <local-package-path>
```

Use the global `--config <PATH>` option when the package belongs to a non-default Asset Hub
configuration.

Installation never mutates the source. It snapshots validated package bytes into same-filesystem
staging, generates a fresh `manifest.lock.json`, verifies the complete package, and only then
installs or replaces `<blob.local.root>/.asset-hub/plugins/<plugin-id>`. Runtime loading is read-only
and uses the same host implementation and limits (1 MiB Manifest, 4 MiB lock, 64 MiB Wasm, and
64 MiB total Web assets). Packages reject symbolic links and special files. Remote Git/GitHub
sources are not currently supported.

### Kind identity and Directory placement

A Kind ID contains two or more lowercase colon-separated segments, for example
`plugin:directory:games:item`. Every segment may contain lowercase ASCII letters, digits, `.`,
`-`, and `_`. Colons are identity separators only: they do not create inheritance. Kind inheritance
continues to use the explicit `parent` field.

A Directory Kind may restrict its direct parent using `allowed_parent_kinds`:

```json
{
  "kind": "plugin:directory:games:item",
  "parent": "core:directory",
  "allowed_parent_kinds": ["plugin:directory:games"],
  "label": "Game"
}
```

A parent Kind can assign otherwise-generic direct children automatically with
`default_child_kind`. The target must be a registered descendant of the declaring Kind and must
allow that Kind as its direct container. This rule applies when a new child would otherwise be
`core:directory`, and when an existing Directory is changed to the parent Kind; only direct
children that are still `core:directory` are reclassified.

```json
{
  "kind": "directory:games",
  "parent": "core:directory",
  "default_child_kind": "directory:games:item",
  "label": "Games"
}
```

An empty or omitted list declares no new constraint. The effective constraint is inherited from
the nearest Kind ancestor that declares a non-empty list; when no declaration exists, any parent is
accepted. The Host enforces this invariant on Directory create, move, and Kind change, including
Kind changes that would invalidate an existing direct child. Allowed entries accept descendants of
the named parent Kind.

The Host deliberately does not model role files such as `README.md`, `HASH.md`, or required
resources in a Directory Kind declaration. Those meanings remain plugin policy. A Directory Action
can inspect the current Directory and decide which names or Resource Kinds are special.

### Providing a singleton Host capability

Resource and directory actions keep provider-owned IDs. An action can implement a singleton Host
capability by declaring its semantic identifier in `provides`:

```json
{
  "id": "example.epub.thumbnail",
  "provides": "thumbnail",
  "label": "EPUB Thumbnail",
  "handler": "render_thumbnail",
  "applies_to": { "kinds": ["example:epub"] },
  "access": "read",
  "output": { "views": ["media"] }
}
```

The Host first filters candidates by content availability, MIME type, extension, and other action
requirements. It then selects the provider declared for the nearest kind in the resource or
directory lineage. A less-specific provider remains the fallback when a more-specific provider is
not actually applicable. Two providers for the same capability at the same nearest kind fail Host
startup instead of being selected by registration or UI sort order.

The Host recognizes `thumbnail` and `text_edit` singleton capabilities for Resource
actions; Directory actions recognize `thumbnail` and `workspace`. A `thumbnail` provider must be read-only,
support the `media` view, and declare the matching `resource_thumbnail` or
`directory_thumbnail` UI location. Read actions that do not need singleton-provider resolution are
ordinary labeled actions without `provides`. A `text_edit` provider must be write and request
`resource.content.replace`; generic `resource.write` and
`resource.derived_asset.write` permissions are not part of the contract. The Host rejects unknown
capability names. Plugins must retain their provider-owned action IDs and must not reuse a `core.*`
action ID.

A Directory `workspace` provider is the exclusive owner of the Host's top-level
`directory_workspace` outlet for the nearest Directory kind. It must be read-only, declare no
effects, support `plugin_frame`, and use `directory_workspace` as its only Host location. The initial invocation
must return that frame; the same Action may additionally declare and return `json` for calls made by
its own frame. When no provider applies, the Host renders `CoreDirectoryWorkspace`. The Core
workspace owns `directory_context_menu`, `directory_thumbnail`, `resource_context_menu`, and
`resource_thumbnail`; those locations are not projected into a plugin workspace. A plugin owns and
implements every layout region or slot inside its iframe.

```json
{
  "id": "example.game.workspace",
  "provides": "workspace",
  "label": "Game Library",
  "handler": "render_workspace",
  "applies_to": { "kinds": ["example:game"] },
  "access": "read",
  "requires": { "children": true, "resources": "metadata" },
  "output": { "views": ["plugin_frame", "json"] },
  "ui": { "locations": ["directory_workspace"] }
}
```

`requires.resources` is `"none"`, `"metadata"`, or `"content"` and defaults to `"none"`.
`metadata` enables the paged directory resource ABI and exposes each Resource's identity, name,
Kind, revision, and content metadata. `content` additionally exposes a call-scoped content handle
for eligible Resources; plugins read that handle with the existing
`asset_hub_content_open/size/read/close` ABI. It requires `directory.resources.list`,
`resource.read`, and `resource.content.read`. Content handles are released with the Directory
Action call and are never filesystem paths. A resource that has no readable content handle still
retains its metadata in the page.

The Directory resource ABI also accepts an optional descendant Directory ID. The Host permits only
the current Action Directory or one of its descendants, so a library workspace can read role files
inside its entries without gaining arbitrary workspace access.

A Resource action normally declares `label`. A singleton capability provider may omit it when the
action targets a child Resource kind; the Host then inherits the normalized label from the nearest
ancestor provider for the same capability. An omitted label is rejected when no ancestor provider
exists. Declaring `label` explicitly remains the way to override the inherited wording. Directory
action labels remain required.

### Returning mutation effects

A write Action declares every effect it may return in `output.effects`. An effect-only Action may
leave `output.views` empty and omit all View fields from its output. Effects request a Host mutation;
the plugin never receives repository, blob-storage, or filesystem authority.

Resource and Directory deletion use the same `delete` effect shape:

```json
{
  "id": "example.plugin.delete",
  "label": "Delete",
  "handler": "delete",
  "access": "write",
  "output": { "effects": ["delete"] }
}
```

The Resource Manifest must allow both `resource.read` and `resource.delete`; a Directory delete
Action uses `directory.read` and `directory.delete`. A successful effect-only handler returns:

```json
{ "effects": [{ "type": "delete" }] }
```

The Host accepts an external delete Action only when the Manifest permission is present and the
matching `plugin.grants.resource_delete` or `plugin.grants.directory_delete` setting is enabled.
Execution still requires the current user to have delete permission for the target aggregate.
Resource deletion uses Host soft-delete semantics; Directory deletion succeeds only for a non-root,
empty directory. A delete effect cannot be combined with another effect in the same declaration or
output.

For bounded scaffolding, a Directory Action can declare `create_tree`. The effect contains canonical
relative Directory paths and base64 Resource snapshots. The Host limits counts and total content,
validates Kind placement and user authorization, rejects paths outside the current Directory, and
rolls back entries created earlier in the effect when a later entry fails. Its Manifest must request
both `directory.create_child` and `resource.create`. Required filenames and template meanings remain
plugin policy rather than Manifest fields.

```json
{
  "effects": [{
    "type": "create_tree",
    "directories": [
      { "path": "game-one", "kind": "directory:games:item" },
      { "path": "game-one/public", "kind": "core:directory" }
    ],
    "resources": [{
      "directory": "game-one",
      "name": "README.md",
      "kind": "example:document",
      "mime_type": "text/markdown; charset=utf-8",
      "encoding": "base64",
      "data": "IyBHYW1lIE9uZQo="
    }]
  }]
}
```

### Plugin Frame Web SDK

The public browser SDK lives in [`web`](web) and hides the Penpal transport used by the Host. A
bundled Web application imports `@asset-hub/plugin-web-sdk`; a plain `index.html` can copy and load
the self-contained `asset-hub-plugin.global.js` build without React, npm, or another framework.
Transport-free Browser Frame constants and types are exported from
`@asset-hub/plugin-web-sdk/contract`; the SDK and Web Host consume that same contract. Rust and
TypeScript contract tests lock its API version, Resource and Directory channels, view kinds, and
Host-normalized effect kinds to one golden document.

```ts
import { connectAssetHubFrame } from "@asset-hub/plugin-web-sdk";

const host = await connectAssetHubFrame();
const output = await host.executeResourceAction("example.plugin.inspect", {});
```

The returned client exposes only:

- `executeResourceAction(action, input?)`, which can call an Action already exposed for the current
  Resource;
- `replaceResourceText(text)`, which is accepted only from the frame created by the current
  Resource's resolved, write `text_edit` provider whose Manifest requests
  `resource.content.replace`;
- `disconnect()`, which releases the frame connection.

A Directory frame uses the separate target-bound client:

```ts
import { connectAssetHubDirectoryFrame } from "@asset-hub/plugin-web-sdk";

const host = await connectAssetHubDirectoryFrame();
const output = await host.executeDirectoryAction("example.game.workspace", {
  operation: "load",
});
```

It can execute an Action already exposed for the bound Directory, request a Directory refresh, and
ask the Host to navigate to a canonical relative Directory path. It cannot address a different
Directory directly or access any `CoreDirectoryWorkspace` slot.

SDK method calls no longer supply Resource identity or request IDs. The Host binds the connection to
the Resource and originating Action that created the frame, while Penpal owns request correlation,
timeouts, errors, and connection cleanup. The Host still validates method arguments, available
Actions, resolved capability, write access, content policy, authorization, and Resource revision.
The plugin runtime and browser frame never receive storage authority.
Resource and Directory execution results both expose nullable views and the names of effects already
applied by the Host. Action input is recursively restricted to bounded JSON objects. A Penpal call
timeout stops the SDK from waiting but does not cancel Host work, so timed-out mutations must be
reconciled against refreshed Host state before any retry.

## Rust API

The main modules are:

- `manifest`: external package authoring models and validation.
- `protocol`: resource and directory action wire types.
- `abi`: versioned Host function definitions and optional guest helpers.

Public types are exported only through their owning module; the crate root keeps
only `CRATE_VERSION` and the three module entry points. Typical plugin code uses
explicit imports:

```rust
use asset_plugin_api::manifest::PluginManifest;
use asset_plugin_api::protocol::{
    PLUGIN_API_VERSION, PluginResourceActionRequest, PluginResourceActionOutput,
};
use asset_plugin_api::abi::content::guest;
```

The SDK deliberately stops at the external contract boundary. The Host converts
Manifest capabilities into its own normalized Action/Kind definitions and maps
its authorization state to `PluginActionAccess` when creating wire requests.
Host executor selection, handler bindings, built-in identifiers, execution
budgets, filesystem paths, and loaded Web assets are not public SDK concepts.

Generate the Rust API documentation locally with:

```bash
cargo doc -p asset-plugin-api --open
```

## Compatibility

Version 4 of the Plugin API and Manifest version 3 are intentionally incompatible with their
predecessors. Existing packages must update their
Manifest and runtime together, migrate Plugin Frame code to the Web SDK, rebuild package artifacts,
and reinstall the rebuilt directory. The Host rejects Manifest version 2 or Plugin API
version 3 packages instead of
translating them.

The principal version 4 / Manifest version 3 changes are:

- Kind IDs accept two or more explicit colon-separated segments; inheritance is still declared by
  `parent` and is never inferred from the ID.
- Directory Kinds may declare direct-parent placement constraints through
  `allowed_parent_kinds`.
- Directory Action resource requirements use `none`, `metadata`, or `content` instead of a boolean.
  Content mode returns call-scoped handles that use the existing content ABI.
- File roles, required filenames, and Resource-to-Directory policy are not Manifest concepts; the
  plugin evaluates them using the Directory Action input and ABI.

Version 3 was intentionally incompatible with version 2. Existing packages had to update their
Manifest and runtime together. Its principal changes were:

- Plugin Frames use the Asset Hub Web Plugin SDK backed by Penpal instead of the former public,
  hand-authored `window.postMessage` envelopes.
- Frame calls are bound to the Resource and originating Action by the Host; callers no longer send
  `plugin_api`, `request_id`, or `resource_id` with each operation.
- Frame calls use Promise results and errors, with connection and method timeouts owned by the SDK.
- The SDK includes both an ESM build and a self-contained browser global build for plain HTML
  plugins.
- Action output contracts declare `output.effects`; effect-only outputs may omit a View. Resource
  and Directory Actions can request the permission-gated `delete` effect.
- Directory kinds may provide one nearest `workspace` capability at the top-level
  `directory_workspace` outlet; that frame replaces `CoreDirectoryWorkspace` instead of receiving
  its internal slots.
- Directory workspace frames use a separate Directory-bound SDK client for exposed Directory
  Actions, refresh, and canonical Host navigation.

Version 2 was intentionally incompatible with version 1. Its principal changes were:

- Resource kinds use `capabilities.resource_kinds`; Directory kinds remain in
  `capabilities.directory_kinds`. At that version, Kind IDs were canonical lowercase
  `namespace:name` values.
- Resource and Directory action contracts use distinct request/output types and canonical
  dot-separated provider IDs such as `example.plugin.inspect`.
- Action metadata includes `description`, uses `access: "read" | "write"`, and declares views in
  `output.views`. Resource MIME matching is declared in `applies_to.mime_types`.
- A Resource singleton-capability provider may omit `label` to inherit the nearest ancestor
  provider's normalized label; ordinary Resource actions and Directory actions still require it.
- Resource and Directory wire snapshots include their current `revision`. Effects are accepted
  only when their aggregate identity and optimistic-concurrency precondition still match.
- Host write-Action requests require `expected_revision`; read Actions may omit it and use the
  latest authorized snapshot, or supply it as an explicit consistency precondition.
- Built-in capabilities remain Host-owned definitions and cannot be declared with a plugin
  runtime discriminator.

Host applications use `asset-core` for normalized Action/Kind and
runtime-independent Action policy, `asset-infra` for Extism execution policy,
and `asset-runtime` for verified Web asset snapshots. External plugin code
should not depend on those workspace-internal crates.

Rust source changes do not require a Manifest or Plugin API version change when
the serialized contract remains unchanged. Changes to document fields,
serialized representations, Host function signatures, or frame messages are
wire-contract changes even when a development release deliberately keeps the
current version value.

Before upgrading, compare the supported values in this README with the
`manifest_version` and `runtime.plugin_api` declared by the plugin.

## Development

Run the crate tests from the repository root:

```bash
cargo test -p asset-plugin-api
```

Contract fixtures are stored in `tests/fixtures`.
