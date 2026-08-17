# Asset Plugin SDK

`asset-plugin-sdk` is the supported Rust authoring SDK and wire contract for external Asset Hub
plugins. Ordinary plugin code imports its high-level runtime contexts directly from the crate root;
Host adapters and advanced integrations may use the lower-level Manifest, protocol, and ABI modules
directly.
Host-owned built-in providers, normalized Action/Kind definitions, execution configuration, and
loaded package snapshots remain outside this crate.

Use this crate when building an Asset Hub plugin runtime or another tool that
needs to read and validate the plugin contract. Applications that only consume
the Asset Hub HTTP API do not need it.

## What It Provides

- Manifest models and validation.
- High-level Resource and Directory Action contexts and response builders.
- Export macros that own Extism entrypoints, wire serialization, and structured failures.
- Bounded content, descendant child Directory, and Directory Resource readers.
- Low-level Resource and Directory action request and response types.
- Singleton resource and directory capabilities with kind-specific providers.
- Structured views, effects, failures, and diagnostics.
- Versioned content and directory Host function definitions.
- Optional Extism guest helpers for Wasm plugins.

## Supported Versions

Asset Hub versions the Rust library and its serialized contracts separately:

| Surface | Current value | Purpose |
| --- | --- | --- |
| Rust crate | `0.1.0` | Rust source API |
| Manifest | `3` | external Extism/Wasm `manifest.json` document format |
| Plugin API | `asset-hub.plugin-api@1` | Action JSON, Host functions, and Plugin Frame Web SDK |

Plugins must declare both `manifest_version` and `runtime.plugin_api`. The host
rejects unsupported contract versions instead of attempting to interpret them.
The only supported runtime discriminator is `"type": "extism"`; `builtin` is
not a plugin runtime value and is rejected during deserialization.

## Getting Started

Add the SDK as the plugin runtime's only Asset Hub/Extism dependency:

```toml
[dependencies]
asset-plugin-sdk = { path = "<path-to-asset-plugin-sdk>", features = ["extism-guest"] }
```

Import the authoring API from the crate root and export a business handler. The SDK decodes the
request, serializes the response, supplies the current Plugin API version, and converts failures
into the structured wire shape:

```rust
use asset_plugin_sdk::{
    Media, ResourceContext, ResourceResponse, Result, export_resource_action,
};

const THUMBNAIL: &str = include_str!("thumbnail.svg");

export_resource_action!(render_thumbnail => render_thumbnail_action);

fn render_thumbnail_action(context: ResourceContext) -> Result<ResourceResponse> {
    let resource = context.resource();
    Ok(ResourceResponse::media(
        Media::base64("image/svg+xml", THUMBNAIL).title(resource.name()),
    ))
}
```

Content access uses the same context regardless of whether the Host supplied inline bytes or an
opaque call-scoped handle. Plugins must choose explicit bounds:

```rust
let bytes = context.content().read_all(128 * 1024 * 1024, 1024 * 1024)?;
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
    "plugin_api": "asset-hub.plugin-api@1"
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
`example:collection:item`. Every segment may contain lowercase ASCII letters, digits, `.`,
`-`, and `_`. Colons are identity separators only: they do not create inheritance. Kind inheritance
continues to use the explicit `parent` field.

A Directory Kind may restrict its direct parent using `allowed_parent_kinds`:

```json
{
  "kind": "example:collection:item",
  "parent": "example:collection",
  "allowed_parent_kinds": ["example:collection"],
  "label": "Collection item"
}
```

A parent Kind can assign otherwise-generic direct children automatically with
`default_child_kind`. The target must be a registered descendant of the declaring Kind and must
allow that Kind as its direct container. This rule applies when a new child would otherwise be
`core:directory`, and when an existing Directory is changed to the parent Kind; only direct
children that are still `core:directory` are reclassified.

```json
{
  "kind": "example:collection",
  "parent": "core:directory",
  "default_child_kind": "example:collection:item",
  "label": "Collection"
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
  "id": "example.collection.workspace",
  "provides": "workspace",
  "label": "Collection workspace",
  "handler": "render_workspace",
  "applies_to": { "kinds": ["example:collection"] },
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

The Directory child and resource ABIs also accept an optional descendant Directory ID. The Host
permits only the current Action Directory or one of its descendants, so a workspace can discover
nested directories and read their resources without gaining arbitrary workspace access.
The high-level SDK exposes this through `children_bounded_in`, `resources_bounded`, and the binary
`DirectoryResource::read_bytes` reader. Omitting the Directory ID keeps both queries bound to the
current Action Directory.

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
      { "path": "item-one", "kind": "example:collection:item" },
      { "path": "item-one/assets", "kind": "core:directory" }
    ],
    "resources": [{
      "directory": "item-one",
      "name": "README.md",
      "kind": "example:document",
      "mime_type": "text/markdown; charset=utf-8",
      "encoding": "base64",
      "data": "IyBJdGVtIE9uZQo="
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
const output = await host.executeDirectoryAction("example.collection.workspace", {
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

- crate root: recommended high-level imports for ordinary plugin runtime code.
- `runtime`: implementation module for Action contexts, bounded Host access, views, responses,
  effects, and export runners.
- `manifest`: external package authoring models and validation.
- `protocol`: resource and directory action wire types.
- `abi`: versioned Host function definitions and optional guest helpers.

Typical plugins should not construct protocol DTOs or invoke raw ABI functions. Those modules
remain public for Host adapters and specialized integrations:

```rust
use asset_plugin_sdk::{
    Media, ResourceContext, ResourceResponse, Result, export_resource_action,
};
```

`ResourceContext::content` hides inline/reference delivery and content-handle lifetime.
`DirectoryContext::children_bounded` and `resources_bounded` hide pagination while rejecting data
sets beyond the plugin-selected limit instead of silently truncating. `Frame` inserts the current
Plugin API version, `Media` and `Tree` own base64 wire encoding, and the export macros return
structured failures. The low-level `protocol` and `abi` modules remain the canonical wire owners.

The Host converts Manifest capabilities into normalized internal Action/Kind definitions and maps
authorization state into wire requests. Host executor selection, handler bindings, built-in
identifiers, execution budgets, filesystem paths, and loaded Web assets are not SDK concepts.

Generate the Rust API documentation locally with:

```bash
cargo doc -p asset-plugin-sdk --open
```

## Versioning

Rust crate version, Manifest document version, and Plugin API version remain independent. The Host
accepts only the current identifiers and does not translate unsupported package versions.

Host applications use `asset-core` for normalized Action/Kind and
runtime-independent Action policy, `asset-infra` for Extism execution policy,
and `asset-runtime` for verified Web asset snapshots. External plugin code
should not depend on those workspace-internal crates.

Rust source changes do not require a Manifest or Plugin API version change when
the serialized contract remains unchanged. Changes to document fields,
serialized representations, Host function signatures, or frame messages are
wire-contract changes even when a development release deliberately keeps the
current version value.

Within `asset-hub.plugin-api@1`, the optional `directory_id` already present in Directory page
requests is supported by both child and resource listing. Omitting it retains the original
action-root behavior; supplying it is an additive descendant query and remains constrained to the
action Directory subtree.

Before upgrading, compare the supported values in this README with the
`manifest_version` and `runtime.plugin_api` declared by the plugin.

## Development

Run the crate tests from the repository root:

```bash
cargo test -p asset-plugin-sdk
```

Contract fixtures are stored in `tests/fixtures`.
