# Asset Hub Plugin Contract

This directory is the committed, language-neutral distribution of Manifest v5 and Plugin API v2.
Do not edit generated JSON files by hand.

- `contract-v2.json` is the discovery catalog for versions, capability IDs, Browser Frame channels
  and method signatures, view/effect discriminants, and Extism Host functions.
- `manifest-v5.schema.json` and `manifest-lock-v5.schema.json` define package documents.
- `*-v2.schema.json` files define Action, ABI JSON, and Browser Frame wire documents.

All JSON schemas use JSON Schema Draft 2020-12 and reject unknown object properties. Extism ABI
function parameters and results are described by `contract-v2.json`; JSON-valued arguments and
results point to their named schemas. Content and directory references are opaque and scoped to one
Action invocation.

The Rust definitions in `asset-plugin-api` are the authoring source. Generate this directory and
the Web SDK constants from the repository root with:

```bash
cargo run -p asset-plugin-api --example export_contract -- \
  asset-plugin-api/spec sdk/asset-web-sdk/src/contract.generated.ts
```
