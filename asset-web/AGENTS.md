# Asset Web agent instructions

These instructions apply to `asset-web` and supplement the repository-level `AGENTS.md`.
Keep this file limited to stable constraints that are easy to violate when changing the browser application.
Do not document transient layouts, colors, copy, or component-level implementation details here.
Read `README.md` for setup and validation commands.

## Architecture boundaries

`asset-web` is the browser application for the global asset workspace. It consumes the core HTTP
API. Preserve these dependency rules:

- Domain code must not depend on React, HTTP, OpenAPI DTOs, or backend transport details.
- Features consume narrow interfaces from `shared/api/gateways.ts`; they must not call `fetch` or
  import `infra/http/generated.ts`.
- Snake-case DTOs and generated OpenAPI types stay inside `infra/http`; domain and feature types use
  camelCase.
- Add an operation only to the gateway whose consumer needs it. Do not create a global all-purpose
  API object.
- `main.tsx` is the only browser composition root.

## State ownership

Keep each state category with its current owner:

- URL path parameters: current directory, pagination, and selected Resource or Directory.
- TanStack Query: server-owned Resource and Directory data.
- React Hook Form or local component state: create, edit, upload, and transient UI state.

Do not introduce a global store that mixes server state, URL state, and form state. Resource and
Directory selection are mutually exclusive. Directory navigation uses paths, while Directory
identity and mutations use stable UUIDs. The browser-visible root path is the empty string.

Resource lifecycle, content, and effective status come from the backend `state` contract. Features
must consume that state directly and must not reconstruct status or precedence from timestamps,
content metadata, or transport fallbacks.

## Resource and Directory mutations

- Mutations go through the relevant gateway and Core use case.
- Resource and Directory updates and deletions carry the current revision as the optimistic
  concurrency precondition. On `concurrency.revision_conflict`, refresh the authoritative snapshot
  before further editing.
- After a successful mutation, update or invalidate the smallest necessary Query cache surface.
- Do not introduce authorization policy in React components.

Current upload facts:

- Uploads hash locally, create or resume a session, send checksum-verified chunks, complete the
  session, and poll until the Resource is published.
- The resume fingerprint includes the complete-file SHA-256. Do not weaken it to filename and size.

Download and delete are core UI capabilities backed by the core REST endpoints
(`GET /resources/{id}/download`, `GET /directories/{id}/download`, `DELETE /resources/{id}`,
`DELETE /directories/{id}`). Directory deletion only succeeds for empty directories; the
confirmation copy must keep saying so.
