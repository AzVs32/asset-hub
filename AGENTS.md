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

## AI-first test policy

During the current AI-led development stage, tests exist only to help an AI agent understand and
safely change the system. They are executable context, not a coverage target, QA inventory, release
gate, or permanent record of every behavior that happens to exist.

Keep a test only when reading or running it gives an AI material information that is not already
obvious from the owning implementation and documentation. The strongest reasons to keep one are:

- a non-obvious domain invariant, state transition, path/identity rule, or security boundary;
- authorization, optimistic concurrency, recovery, compensation, or failure ordering;
- a small representative persistence, migration, streaming, or atomic-filesystem guarantee;
- a public HTTP, OpenAPI, Plugin API, Manifest, ABI, golden-wire, or frame-host compatibility boundary;
- a concise regression for a real bug whose cause would otherwise be easy for an AI to reintroduce.

Delete or do not add tests that primarily create context noise, including:

- getters, setters, builder assignments, constant/default mirroring, or direct DTO field copying;
- framework behavior such as ordinary Clap/React/router parsing and rendering;
- ordinary CRUD success paths already demonstrated at another layer;
- repeated valid/invalid permutations that add no new rule;
- presentation details while the UI is still changing quickly;
- tests whose fixture, mock, or fake setup is substantially harder to understand than the invariant;
- a second assertion of a rule already expressed more clearly by an owning-layer test or golden contract.

Count test support code as part of the cost. Do not introduce a shared fake framework merely to make
more tests convenient. Prefer one small scenario at the layer that owns the behavior. When a test
needs broad setup, keep it only for a high-risk cross-component sequence that cannot be expressed
more clearly in documentation or a narrower test.

Before adding or retaining a test, be able to state in one sentence what future AI mistake it prevents.
If that sentence only says that the implementation should keep doing what it currently does, remove the
test. Do not automatically add tests for every production change, and do not replace deleted low-value
tests with equivalent snapshots or parameterized cases.

## Validation

After making changes:

- run formatting and lint checks for every affected language;
- run tests for the affected crates or packages;
- run broader workspace checks when changing public contracts, shared ports, migrations, 
  runtime assembly, or plugin protocols;
- do not claim validation succeeded unless the command was actually run.
