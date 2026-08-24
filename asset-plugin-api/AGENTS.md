# asset-plugin-api instructions

## Responsibility

This crate is the canonical runtime- and language-neutral external Plugin API. It owns Manifest
models and validation, Action JSON protocols, diagnostics, effects, views, Wasm Host function names,
and ABI value objects. It must not depend on Extism, another Wasm runtime, or an authoring SDK.

Manifest, protocol, and ABI changes are public wire-contract changes. Update Serde models,
validation, golden fixtures, Host consumers, and supported version references together.

Run `cargo test -p asset-plugin-api` after changes.
