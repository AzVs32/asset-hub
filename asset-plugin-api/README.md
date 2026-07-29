# Asset Plugin API compatibility policy

`asset-plugin-api` contains three independently versioned surfaces. Host/guest runtime contracts
share one Plugin API version.

| Surface | Current | Carried by | Changes when |
| --- | --- | --- | --- |
| Rust crate | `0.1.0` | Cargo package declaration and `CRATE_VERSION` | The Rust source API changes |
| Manifest | `1` | `manifest_version` | The authoring document structure or declaration semantics break |
| Plugin API | `asset-hub.plugin-api@1` | `runtime.plugin_api` | Action JSON, Host functions, or Plugin Frame messages break |

## Rust crate version

The Cargo version describes source compatibility for Rust plugin authors. Before `1.0`, a minor
version may contain source-breaking changes; patch versions remain source compatible. Plugin
packages should pin the crate version used to build their Wasm artifact. Changing only Rust type
names or constructors does not require a Manifest or wire-protocol bump when serialized JSON stays
unchanged. The current contract is intentionally baselined at `0.1.0`; pre-baseline development
versions are not supported or documented as compatibility targets.

## Manifest version

Manifest V1 is the only accepted authoring format. The host rejects other versions. Package
entries are convention-based: Extism uses `plugin.wasm`, and an optional Web UI uses root
`index.html`; neither path is configurable in the Manifest. The host generates a missing package
lock on first startup and only verifies an existing lock thereafter. The generated lock uses one
flat path-to-digest map without runtime or Web groups:

```json
{
  "manifest_version": 1,
  "plugin_id": "example.plugin",
  "integrity": {
    "plugin.wasm": "<sha256>",
    "index.html": "<sha256>",
    "assets/app.js": "<sha256>"
  }
}
```

Keys are paths relative to the plugin package. `manifest.json` and `manifest.lock.json` are metadata
and are not included in `integrity`.

## Plugin API version

The Plugin API is the single compatibility boundary between Host and executable plugin packages.
It versions Action handler JSON, Content and Directory Host functions, and Plugin Frame
`postMessage` messages. The host accepts only `asset-hub.plugin-api@1`, and every Extism plugin
must declare that value explicitly. A `plugin_frame` view and every message sent through its frame
bridge carry that same `plugin_api` value.

## Action ID convention

Action IDs are extensible and their naming convention is not enforced by the wire protocol.
Authors should use `<plugin-id>.<verb>` so globally registered actions remain distinct and their
owner is clear. For example, the built-in resource download action is
`core.resource.download`, while bundled plugins contribute actions such as
`azvs.markdown.render` and `azvs.epub.cover`.

Use a stable lowercase ID for protocol calls and keep user-facing text in the action `label`.
Plugins that expose several subjects may add another segment, such as
`example.media.image.convert`.

## Content Host functions

Non-inline content is represented by an opaque reference. Host function names, signatures, range
semantics, maximum-read behavior, and handle ownership/lifetime belong to the Plugin API. Content
references do not carry another ABI version.

## Directory actions and Host API

Resource and directory actions share the normalized action shell (ID, label, handler, access,
executor, views, and UI locations), while keeping target contracts separate. Resource actions own
content matching, delivery requirements, and `replace_content`; directory actions own kind matching,
children/resources requirements, and constrained `update` or `create_child` effects.

A directory handler receives one aggregate snapshot plus an opaque, call-scoped `directory_ref`.
It can page direct children through `asset_hub_directory_list_children` and direct resources through
`asset_hub_directory_list_resources`. The host validates `directory.children.list` and
`directory.resources.list` independently, caps each page at 100 items, and invalidates the reference
when the action call ends. The reference cannot select another directory or request a whole subtree.
These Host functions are versioned by the same Plugin API.

## Release checklist

For every protocol change:

1. Classify the change against the three surfaces and bump only the affected versions.
2. Update Manifest Serde and host-validation tests when relevant.
3. Replace the JSON golden fixtures with the new current wire contract.
4. Build bundled plugins against the new Rust crate and verify their generated package locks.
5. Document the exact Manifest and Plugin API versions accepted by the host.
