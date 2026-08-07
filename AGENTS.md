# Asset Hub repository instructions

These instructions apply to the entire repository.
A nested `AGENTS.md` supplements these rules for its subtree.
When rules conflict, the nearest applicable `AGENTS.md` takes precedence.

Update affected documentation in the same change when behavior, architecture,
public contracts, configuration, or operational procedures change.

Before editing a module:

1. Read this file and the nearest nested `AGENTS.md`.
2. Read the module README or architecture document referenced by that file.
3. Distinguish current implementation facts from planned extension points.
4. Do not describe a planned backend or extension point as supported until 
   its runtime wiring, tests, configuration, and documentation are complete.

## Project model

Asset Hub is a local-first asset management system implemented with
hexagonal architecture and a microkernel-style plugin system.

The important aggregates are:

- `Resource`: an asset and its metadata/content reference.
- `Directory`: an independent hierarchy aggregate identified by a stable UUID.
- `User`: identity, role, status, and workspace boundary.

## Repository map

- `asset-plugin-api`: public plugin authoring and wire-contract crate.
- `asset-core`: workspace-internal domain, ports, and application services.
- `asset-infra`: concrete SQLx, OpenDAL, filesystem, registry, manifest, and Extism adapters.
- `asset-runtime`: reusable runtime assembly and background-task ownership.
- `asset-http`: Axum transport, authentication, DTOs, OpenAPI, and HTTP executable.
- `asset-cli`: administration commands and CLI executable.
- `asset-web`: React host using domain/application/adapter boundaries.
- `plugins`: bundled external plugins that consume `asset-plugin-api`.
- `docker`: production image and local Compose assembly.

## Dependency rules

MUST:

- keep domain and service logic independent of Axum, SQLx, OpenDAL, Extism runtime objects, 
  and CLI parsing;
- define host requirements as ports in `asset-core::port` and implement them in adapters;
- keep external plugin contracts in `asset-plugin-api`;
- use `ResourceService::secured` or an equivalently authorization-bound core use case for
  user-scoped or untrusted resource mutations; trusted local maintenance commands must remain
  explicit administrative operations;
- preserve a single composition root for each executable surface.

MUST NOT:

- make plugin runtimes depend on `asset-core`, `asset-infra`, `asset-runtime`, `asset-http`,
  or `asset-cli`;
- expose SQLx pools, OpenDAL operators, filesystem paths, Extism handles, or HTTP DTOs through
  core domain APIs;
- duplicate authorization policy in a transport or repository adapter;
- bypass core services by mutating repositories or blob storage directly from handlers or commands;
- re-export plugin API types through unrelated host crates merely for convenience.

## Current implementation facts

Treat the following as facts until the implementation and documentation are changed together:

- database backend: SQLite only;
- blob backend: local filesystem through OpenDAL only;
- HTTP session store: SQLite;
- PostgreSQL migrations directory: placeholder only;
- plugin runtime: Extism/Wasm;
- root directory ID: nil UUID `00000000-0000-0000-0000-000000000000`.

## Validation

After making changes:

- run formatting and lint checks for every affected language;
- run tests for the affected crates or packages;
- run broader workspace checks when changing public contracts, shared ports, migrations, 
  runtime assembly, or plugin protocols;
- do not claim validation succeeded unless the command was actually run.
