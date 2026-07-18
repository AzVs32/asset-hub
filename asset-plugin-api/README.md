# Asset Plugin API compatibility policy

`asset-plugin-api` contains four independently versioned surfaces. A release must not infer one
version from another.

| Surface | Current | Carried by | Changes when |
| --- | --- | --- | --- |
| Rust crate | `0.3.0` | Cargo package declaration and `CRATE_VERSION` | The Rust source API changes |
| Manifest | `3` | `manifest_version` | The authoring document structure or declaration semantics break |
| Plugin JSON API | `asset-hub.plugin-api@0.3` | `runtime.plugin_api` | Handler request, success output, or failure wire contracts break |
| Content ABI | `1` | `content_ref.abi_version` | A host function signature, handle lifecycle, or range-read contract breaks |

## Rust crate version

The Cargo version describes source compatibility for Rust plugin authors. Before `1.0`, a minor
version may contain source-breaking changes; patch versions remain source compatible. Plugin
packages should pin the crate version used to build their Wasm artifact. Changing only Rust type
names or constructors does not require a Manifest or wire-protocol bump when serialized JSON stays
unchanged.

### Rust crate 0.2 migration

The generic `PluginContentEncoding` was replaced with context-specific enums while preserving the
existing JSON strings:

- inline request bytes use `PluginInlineContentEncoding::Base64`;
- content references use `PluginContentReferenceEncoding::Handle`;
- media views use `PluginMediaEncoding::{Base64, Url}`;
- replacement effects use `PluginReplacementEncoding::Base64`.

`PluginContentRange` fields are now private so overflow validation cannot be bypassed. Construct a
range with `PluginContentRange::new` and read it through `offset()`, `length()`, and `end()`.

### Rust crate 0.3 migration

Resource paths now have one source of truth. `PluginResource.directory` and
`PluginResource.name` identify the object path; `PluginResourceContent` no longer repeats `key` or
`original_filename`. Content replacement effects preserve the resource path and therefore no
longer accept `original_filename`. `PluginResourceContent.checksum` contains the single checksum
calculated by the host. Replacement effects cannot submit checksum values; the host calculates a
new checksum from the returned content bytes.

## Manifest version

Manifest V3 is the canonical authoring format and is the only format described by the embedded
JSON Schema and starter template. The runtime currently accepts V2 as a migration input and
normalizes its aliases and permissions to the V3 Rust representation. Compatibility-only V2
syntax is intentionally not valid against the V3 schema.

Additive optional fields may remain in the current Manifest version. Removing or renaming fields,
adding required fields, or changing declaration semantics requires a new Manifest version and a
new schema document. The host must validate the declared version before registering capabilities.

## Plugin JSON API version

The plugin API versions the JSON passed to and returned from action handlers. Pre-1.0 minor
versions are explicit protocol levels, not a loose SemVer range. The host currently supports only
`asset-hub.plugin-api@0.3`; omitted `runtime.plugin_api` values default to that level.

Adding an omitted-by-default optional field may remain compatible. Renaming fields, changing enum
strings, changing defaults, or making data required requires a new plugin API level. Every level
must retain request, success-output, and failure golden fixtures. A host may remove an old level
only after its documented compatibility window ends.

## Content ABI version

The content ABI versions the Extism host functions used for non-inline object bytes. It is
independent of the JSON API because the JSON contains only an opaque reference and its
`abi_version`. Changing a host function name or signature, range semantics, maximum-read behavior,
or handle ownership/lifetime requires a new ABI version. Guests must reject unknown ABI versions
before calling host functions.

## Release checklist

For every protocol change:

1. Classify the change against all four surfaces and bump only the affected versions.
2. Update the Manifest schema/template and the Schema–Serde–host conformance tests when relevant.
3. Update JSON golden fixtures for every supported plugin API level.
4. Build bundled plugins against the new Rust crate and verify their sealed packages.
5. Document the oldest Manifest, plugin API, and content ABI versions accepted by the host.
