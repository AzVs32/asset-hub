# Asset Plugin API

`asset-plugin-api` is the public Rust contract for extending Asset Hub. It
provides the shared types used to describe a plugin, declare its capabilities,
exchange action data with the host, and access supported host functions from
Wasm.

Use this crate when building an Asset Hub plugin runtime or another tool that
needs to read and validate the plugin contract. Applications that only consume
the Asset Hub HTTP API do not need it.

## What It Provides

- Manifest models and validation.
- Resource and directory action request and response types.
- Structured views, effects, failures, and diagnostics.
- Versioned content and directory Host function definitions.
- Optional Extism guest helpers for Wasm plugins.

## Supported Versions

Asset Hub versions the Rust library and its serialized contracts separately:

| Surface | Current value | Purpose |
| --- | --- | --- |
| Rust crate | `0.1.0` | Rust source API |
| Manifest | `1` | `manifest.json` document format |
| Plugin API | `asset-hub.plugin-api@1` | Action JSON, Host functions, and Plugin Frame messages |

Plugins must declare both `manifest_version` and `runtime.plugin_api`. The host
rejects unsupported contract versions instead of attempting to interpret them.

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
  "manifest_version": 1,
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
    "resource_actions": [
      {
        "id": "example.plugin.inspect",
        "label": "Inspect",
        "handler": "inspect",
        "applies_to": {
          "kinds": ["core:resource"]
        },
        "views": ["json"]
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

## Rust API

The main modules are:

- `manifest`: authoring models, validation, and normalization.
- `protocol`: resource and directory action wire types.
- `abi`: versioned Host function definitions and optional guest helpers.
- `domain`: normalized action definitions.
- `policy`: plugin execution limit values.

Common types and version constants are also re-exported from the crate root.
Generate the Rust API documentation locally with:

```bash
cargo doc -p asset-plugin-api --open
```

## Compatibility

Rust source changes do not require a Manifest or Plugin API version change when
the serialized contract remains unchanged. Changes to document fields,
serialized representations, Host function signatures, or frame messages may
require a corresponding contract version update.

Before upgrading, compare the supported values in this README with the
`manifest_version` and `runtime.plugin_api` declared by the plugin.

## Development

Run the crate tests from the repository root:

```bash
cargo test -p asset-plugin-api
```

Contract fixtures are stored in `tests/fixtures`.
