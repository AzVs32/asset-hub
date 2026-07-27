# Asset Plugin API compatibility policy

`asset-plugin-api` contains four independently versioned surfaces. A release must not infer one
version from another.

| Surface | Current | Carried by | Changes when |
| --- | --- | --- | --- |
| Rust crate | `0.4.0` | Cargo package declaration and `CRATE_VERSION` | The Rust source API changes |
| Manifest | `3` | `manifest_version` | The authoring document structure or declaration semantics break |
| Plugin JSON API | `asset-hub.plugin-api@0.4` | `runtime.plugin_api` | Handler request, success output, or failure wire contracts break |
| Content ABI | `1` | `content_ref.abi_version` | A host function signature, handle lifecycle, or range-read contract breaks |
| Directory Host API | `1` | `DIRECTORY_HOST_API_VERSION` | Directory pagination functions, cursors, or page shapes break |

## Rust crate version

The Cargo version describes source compatibility for Rust plugin authors. Before `1.0`, a minor
version may contain source-breaking changes; patch versions remain source compatible. Plugin
packages should pin the crate version used to build their Wasm artifact. Changing only Rust type
names or constructors does not require a Manifest or wire-protocol bump when serialized JSON stays
unchanged.

### Rust crate 0.4 contract

`BinaryUrlView` was replaced by `DownloadView`, and the serialized view discriminator changed from
`binary_url` to `download`. The new view has one meaning: a host-owned URL that should be downloaded,
not previewed according to its MIME type. Plugins should return `MediaView` for display or playback.

## Manifest version

Manifest V3 is the only accepted authoring format. The host rejects other versions and does not
normalize removed field names or permission shapes.

## Plugin JSON API version

The plugin API versions the JSON passed to and returned from action handlers. The host accepts only
`asset-hub.plugin-api@0.4`, and every Extism plugin must declare that value explicitly. Request,
success-output, and failure golden fixtures lock the current wire contract.

## Action ID convention

Action IDs are extensible and their naming convention is not enforced by the wire protocol.
Authors should use `<plugin-id>.<verb>` so globally registered actions remain distinct and their
owner is clear. For example, the built-in resource download action is
`core.resource.download`, while bundled plugins contribute actions such as
`azvs.markdown.render` and `azvs.epub.cover`.

Use a stable lowercase ID for protocol calls and keep user-facing text in the action `label`.
Plugins that expose several subjects may add another segment, such as
`example.media.image.convert`.

## Content ABI version

The content ABI versions the Extism host functions used for non-inline object bytes. It is
independent of the JSON API because the JSON contains only an opaque reference and its
`abi_version`. Changing a host function name or signature, range semantics, maximum-read behavior,
or handle ownership/lifetime requires a new ABI version. Guests must reject unknown ABI versions
before calling host functions.

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

## Release checklist

For every protocol change:

1. Classify the change against all five surfaces and bump only the affected versions.
2. Update Manifest Serde and host-validation tests when relevant.
3. Replace the JSON golden fixtures with the new current wire contract.
4. Build bundled plugins against the new Rust crate and verify their sealed packages.
5. Document the exact Manifest, plugin API, and content ABI versions accepted by the host.
