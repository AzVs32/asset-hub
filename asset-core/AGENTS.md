# asset-core instructions

## Responsibility

`asset-core` is the workspace-internal domain and application kernel. It is not the plugin SDK.

- `domain`: validated business values and aggregates.
- `port`: semantic requirements implemented by outer adapters.
- `service`: application use cases coordinating domain objects and ports.
- `error`: typed error classification consumed by workspace applications.

## Domain rules

MUST:

- keep domain types independent of SQL schemas, filesystem paths, and runtime handles;
- enforce intrinsic invariants in constructors/value types;
- preserve stable IDs for `Resource`, `Directory`, `User`, and `UploadSession`;
  preserve `StorageKey` as content identity rather than replacing it with a filesystem path;
- keep `Directory` an independent aggregate; derive paths through directory services/indexes;
- model object content as metadata plus a storage reference, never as repository-owned file bytes.

MUST NOT:

- import `sqlx`, `opendal`, `axum`, `extism`, or application entry crates;
- add transport-specific serialization fields solely to satisfy one adapter;
- use a path string as the authorization identity of a directory or resource.

## Port rules

A port describes what Core needs, not how an adapter works.

MUST:

- expose each SPI type through the curated `asset_core::port` surface;
- use `asset-plugin-api` contract types only at plugin-facing boundaries;
- keep implementation modules private and re-export only the intended SPI;
- document ordering, atomicity, ownership, streaming, and failure guarantees when they matter;
- design storage and scan ports so scanners report facts and services decide mutations.

MUST NOT expose through a port:

- concrete database pools or transactions;
- OpenDAL operators;
- Extism plugin/runtime objects;
- concrete filesystem watcher types.
- adapter configuration structures.

## Service rules

MUST:

- place cross-aggregate workflow, authorization, compensation, and persistence ordering in services;
- resolve the target directory/resource before authorization when the target determines scope;
- route untrusted resource operations through `SecuredResourceService`;
- keep per-object concurrency controls keyed by stable `StorageKey` or aggregate ID;
- preserve streaming for large content and uploads.

MUST NOT move business policy into transport, command, persistence, or plugin-runtime adapters.

## Errors

- Extend typed core errors when callers need stable classification.
- Preserve conflict/not-found/validation/authorization distinctions.
- Attach operation context when propagating a port failure, without exposing secrets or object bytes.
- Do not use `anyhow` in this crate.

## Tests

- Put pure value and aggregate tests beside their module.
- Use fake ports for service tests and assert call ordering/compensation where relevant.
- Add authorization tests for administrator, member workspace, descendant access, and outside-scope
  denial when changing secured use cases.
- Add concurrency or recovery tests when changing upload finalization, replacement, or reconciliation.

Run:

```bash
cargo test -p asset-core
```
