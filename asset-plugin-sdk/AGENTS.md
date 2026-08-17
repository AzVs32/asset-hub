# asset-plugin-sdk instructions

## Responsibility

This crate is the only supported Rust SDK and wire contract for external plugin authors. It defines
Manifest models, Host/plugin JSON protocols, Wasm Host ABI helpers, Plugin Frame contracts, and
diagnostics. Its `runtime` module implements the narrow high-level authoring surface re-exported at
the crate root for ordinary Extism guests. Host-normalized action/kind models, built-in
providers, execution policy, loaded package snapshots, and application-facing DTOs belong to Host
crates.

Read `asset-plugin-sdk/README.md` before changing any public or serialized type.

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
- expose public items through their canonical `manifest`, `protocol`, or `abi` owner module;
- keep author-facing wrappers implemented in `runtime` and curate their common imports as
  crate-root re-exports;
- keep wire encoding, structured failure conversion, opaque references, and pagination cursors out
  of ordinary plugin business code;
- keep resource and directory action wire contracts separate;
- use stable lowercase action IDs and recommend `<plugin-id>.<verb>`.

MUST NOT:

- expose SQLx, OpenDAL, Axum, host filesystem, normalized Host models, built-in identifiers,
  execution configuration, loaded package assets, or internal aggregate implementation types;
- add crate-root wire-type re-exports or compatibility module aliases;
- silently accept unknown protocol versions;
- change defaulting or optional-field behavior without compatibility tests;
- couple the public contract to consumer-specific behavior.

## Security and limits

- Treat all decoded Manifest, action input/output, diagnostics, effects, and frame messages as
  untrusted.
- Keep wire and ABI range constraints explicit and validated.
- Effects describe requested host mutations; plugins never receive authority to apply them directly.
- Content references and directory references are opaque, scoped handles.

## Required updates for contract changes

MUST update together as applicable:

- Serde models and validation;
- Host-side Manifest conversion adapters;
- JSON golden fixtures in `tests/fixtures`;
- host-side adapter tests;
- `asset-plugin-sdk/README.md` supported-version table and compatibility notes.

Run:

```bash
cargo test -p asset-plugin-sdk
```
