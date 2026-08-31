# Asset Web agent instructions

These instructions apply to `asset-web` and supplement the repository-level `AGENTS.md`.
Keep this file limited to stable constraints that are easy to violate when changing the browser host.
Do not document transient layouts, colors, copy, or component-level implementation details here.
Read `README.md` for setup and validation commands.

## Architecture boundaries

`asset-web` is the authenticated browser host for the asset workspace, user administration, and
plugin UI. Preserve these dependency rules:

- Domain code must not depend on React, HTTP, OpenAPI DTOs, or backend transport details.
- Features consume narrow interfaces from `shared/api/gateways.ts`; they must not call `fetch` or
  import `infra/http/generated.ts`.
- Snake-case DTOs and generated OpenAPI types stay inside `infra/http`; domain and feature types use
  camelCase.
- Add a capability only to the gateway whose consumer needs it. Do not create a global all-purpose
  API object.
- `main.tsx` is the only browser composition root.

## State ownership

Keep each state category with its current owner:

- URL path/search parameters: current directory, search, kind filter, pagination, and selected
  Resource or Directory.
- TanStack Query: server-owned Resource, Directory, Kind, User, authorization, and session data.
- React Hook Form or local component state: create, edit, upload, and transient UI state.
- Session Context: current authenticated user.
- Plugin Kernel: renderer, host-slot, action-provider, and workspace-provider selection.

Do not introduce a global store that mixes server state, URL state, and form state. Resource and
Directory selection are mutually exclusive. Directory navigation uses paths, while Directory
identity and mutations use stable UUIDs. The browser-visible root path is the empty string.

Resource lifecycle, content, and effective status come from the backend `state` contract. Features
must consume that state directly and must not reconstruct status or precedence from timestamps,
content metadata, or transport fallbacks.

## Resource and Directory mutations

- Mutations go through the relevant gateway and backend authorization-bound use case.
- Resource and Directory updates carry the current revision. On
  `concurrency.revision_conflict`, refresh the authoritative snapshot before further editing.
- After a successful mutation, update or invalidate the smallest necessary Query cache surface.
- Do not reproduce authorization policy or Kind hierarchy rules in React components.

Current upload facts:

- Browser uploads do not submit a Resource Kind. The backend detects it from MIME metadata and the
  final Resource path, falling back to `core:resource` when no Kind matcher applies.
- Uploads hash locally, create or resume a session, send checksum-verified chunks, complete the
  session, and poll until the Resource is published.
- The resume fingerprint includes the complete-file SHA-256. Do not weaken it to filename and size.

## Plugin-host invariants

- Do not hardcode plugin IDs, plugin-specific Resource/Directory Kinds, or plugin Action IDs in the
  host.
- The backend returns the applicable Actions and resolved singleton providers. The frontend selects
  only among those returned contracts.
- Automatic thumbnail providers must be resolved, read-only `thumbnail` providers. Never execute a
  write Action automatically.
- A Directory `workspace` provider replaces the entire Core workspace subtree. When it is active,
  Core list, detail, row-menu, and thumbnail slots inside that subtree do not exist.
- Breadcrumb navigation and the Directory Kind editor belong to the outer Host Shell and remain
  available when a plugin owns the workspace.
- Plugin media and frame URLs must resolve to approved backend plugin paths. Do not load arbitrary
  external URLs.
- Resource frames remain bound to their initial Resource ID. Only the write `edit` provider that
  produced a frame may replace that Resource's text.
- Directory frames remain bound to their initial Directory ID and may access only host-authorized
  direct children through opaque IDs.
- Keep JSON input bounded and reject cyclic, non-finite, non-JSON, or excessively deep values before
  passing input to a plugin gateway.

Changes to frame protocol versions, Host methods, view kinds, capability IDs, or host-slot semantics
must update the Web SDK, host implementation, contract tests, and stable documentation together.
