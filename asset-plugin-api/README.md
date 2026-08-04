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
- Structured views, effects, failures, and diagnostics.
- Versioned content and directory Host function definitions.
- Optional Extism guest helpers for Wasm plugins.

## Supported Versions

Asset Hub versions the Rust library and its serialized contracts separately:

| Surface | Current value | Purpose |
| --- | --- | --- |
| Rust crate | `0.1.0` | Rust source API |
| Manifest | `1` | external Extism/Wasm `manifest.json` document format |
| Plugin API | `asset-hub.plugin-api@1` | Action JSON, Host functions, and Plugin Frame messages |

Plugins must declare both `manifest_version` and `runtime.plugin_api`. The host
rejects unsupported contract versions instead of attempting to interpret them.
The only supported runtime discriminator is `"type": "extism"`; `builtin` is
not a plugin runtime value and is rejected during deserialization.

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

The host requires a sealed package before startup. After assembling the package
under a directory whose name equals `plugin.id`, generate and verify its lock:

```bash
asset plugin --generate-lock path/to/<plugin-id>/manifest.json
asset plugin --verify path/to/<plugin-id>/manifest.json
```

Lock generation writes `manifest.lock.json` only when it is absent. Verification
and runtime loading are read-only and use the same host implementation and limits
(1 MiB Manifest, 4 MiB lock, 64 MiB Wasm, and 64 MiB total Web assets). Packages
reject symbolic links and undeclared files. Rebuilds must remove the old lock,
generate a new one, and verify it before startup.

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
    PLUGIN_API_VERSION, PluginActionRequest, PluginActionOutput,
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

This development line intentionally narrows Manifest `1` in place: older
documents that declare `runtime.type = "builtin"` are incompatible and must not
be used as external packages. Built-in capabilities are assembled by the Host
without a plugin Manifest.

The responsibility change has these compatibility effects:

- Rust API: breaking; `PluginRuntime::Builtin`, Host-normalized Action types,
  built-in Action identifiers, `PluginExecutionPolicy`, and `PluginWebAssets`
  were removed. Wire request access is now named `PluginActionAccess`. Root-level
  type re-exports and compatibility module aliases were removed; use `manifest`,
  `protocol`, and `abi` paths. `PLUGIN_API_VERSION` now belongs to `protocol`.
- Manifest `1`: breaking for documents that used the Host-only `builtin` value;
  every package now requires an Extism runtime and `plugin.wasm` integrity entry.
- Plugin API `asset-hub.plugin-api@1`: unchanged; action JSON, Host functions,
  and Plugin Frame messages retain their existing wire shapes.

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
