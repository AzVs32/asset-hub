# Asset Web

`asset-web` is the browser application for Asset Hub. It opens directly to the global asset
workspace for directory browsing, resource upload, and resource management, speaking only to the
core HTTP API.

AI agents and maintainers should follow [`AGENTS.md`](AGENTS.md) for dependency rules and state
ownership.

## Prerequisites

- Node.js 22.12 or later.
- `asset-http` running on `http://127.0.0.1:8080`.

## Quick start

```bash
npm ci
npm run dev
```

The development server listens on `http://127.0.0.1:5173` and proxies `/api` to the local API.

To use a different API origin:

```bash
VITE_API_BASE_URL=http://127.0.0.1:8080 npm run dev
```

## Commands

| Command | Purpose |
| --- | --- |
| `npm run dev` | Start the local development server |
| `npm run check` | Run formatting/lint checks and TypeScript validation |
| `npm test` | Run available package tests |
| `npm run build` | Create a production build |
| `npm run generate:api` | Regenerate HTTP-only OpenAPI declarations from a running API |

## Browser architecture

- `main.tsx` assembles the HTTP gateways and Query client; `app/` owns providers, routing, and
  the route error boundary.
- `domain/` contains Resource and Directory models and directory path rules. Browser upload
  inputs and publication results live beside the gateway contracts in `shared/api/`.
- `features/asset-workspace/` owns the workspace, its query/mutation hooks, and its components.
  `workspace-cache.ts` scopes invalidation to affected directories and refreshes resource snapshots.
- `infra/http/` owns HTTP and OpenAPI mapping and resumable upload transport. `infra/crypto/`
  owns incremental hashing and its worker.

Directory navigation uses the raw URL pathname, decoded once. Pagination and mutually exclusive
Resource/Directory selection use query parameters. Switching directories clears the previous
listing; deleting the last item on a page returns to the last valid page. Resource details use a
side panel on desktop and a drawer on small screens.

Edits retain the snapshot they started from. Background refreshes do not overwrite drafts; a
changed revision blocks further editing until the user explicitly reloads the latest snapshot.
Revision conflicts refresh the authoritative resource before retrying. Failed detail refreshes
keep editing blocked and expose a retry action.

## Upload recovery

Uploads hash the complete file locally and retain checksum-verified chunk resume information in
browser local storage. The workspace restores stored sessions on reopening. Incomplete transfers
require choosing the same file, name, and destination again; files are not retained by the browser
application. Publication status is owned by Query, with cancellable requests and automatic
backoff/rechecking on connection errors. A connection error does not imply publication failure.
Completed sessions are acknowledged only after the published resource reaches the Query cache.
If local storage is unavailable, the current upload still works, but reload recovery is unavailable.

Choosing the same file after a terminal publication failure removes the failed session and its
staging data, then starts a new upload. If cleanup fails, the saved session remains available for
another retry. The workspace removes the replaced session's stale status entry. Temporary status
request failures continue to preserve the existing resumable session.

The small regression suite covers URL/path identity, directory transitions, optimistic concurrency,
draft preservation, and upload recovery. `npm test` fails if no tests are found. For production,
configure the host to serve `index.html` for directory paths and proxy `/api` to `asset-http` (or
set `VITE_API_BASE_URL` at build time); Vite's development proxy is not part of the built files.
