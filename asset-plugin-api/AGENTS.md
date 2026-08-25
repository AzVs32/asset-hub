# asset-plugin-api instructions

## Responsibility

This crate is the canonical runtime- and language-neutral external Plugin API. It owns Manifest
models and validation, Action JSON protocols, diagnostics, effects, views, Wasm Host function names,
and ABI value objects. It must not depend on Extism, another Wasm runtime, or an authoring SDK.

Manifest, protocol, and ABI changes are public wire-contract changes. Update Serde models,
validation, generated `spec/` artifacts, fixtures, Host consumers, Web SDK generated constants,
and supported version references together. Unknown fields are rejected on every versioned object.
Host code accepts `ValidatedPluginManifest`, not an unvalidated document.

Run `cargo test -p asset-plugin-api` after changes.
