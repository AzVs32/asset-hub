# asset-plugin-api instructions

## Responsibility

This crate is the only supported contract for external plugin authors. It defines Manifest models,
normalized action definitions, Host/plugin JSON protocols, Wasm Host ABI helpers, Plugin Frame
contracts, diagnostics, and execution policy values.

Read `asset-plugin-api/README.md` before changing any public or serialized type.

## Compatibility surfaces

Three versions are independent:

1. Rust crate version.
2. Manifest document version.
3. Unified Plugin API version covering Action JSON, Host functions, and frame messages.

MUST classify every change against these surfaces before implementation.

A Rust rename with unchanged serialized JSON can be crate-only. A field name, enum representation,
required field, Host function signature, range rule, frame message, or semantic behavior change is a
wire-contract change.

## Public API rules

MUST:

- keep this crate independent of all host implementation crates;
- keep `extism-pdk` optional behind `extism-guest`;
- use explicit Serde names and validation for wire data;
- reject unsupported Manifest and Plugin API versions explicitly;
- preserve compatibility re-exports unless a deliberate breaking release removes them;
- keep resource and directory action target contracts separate even when they share a normalized
  shell;
- use stable lowercase action IDs and recommend `<plugin-id>.<verb>`.

MUST NOT:

- expose SQLx, OpenDAL, Axum, host filesystem, or internal aggregate implementation types;
- silently accept unknown protocol versions;
- change defaulting or optional-field behavior without compatibility tests;
- couple the public contract to consumer-specific behavior.

## Security and limits

- Treat all decoded Manifest, action input/output, diagnostics, effects, and frame messages as
  untrusted.
- Keep resource limits explicit and serializable.
- Effects describe requested host mutations; plugins never receive authority to apply them directly.
- Content references and directory references are opaque, scoped handles.

## Required updates for contract changes

MUST update together as applicable:

- Serde models and validation;
- normalized domain conversion;
- JSON golden fixtures in `tests/fixtures`;
- host-side adapter tests;
- `asset-plugin-api/README.md` supported-version table and compatibility notes.

Run:

```bash
cargo test -p asset-plugin-api
```
