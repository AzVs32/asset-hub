# Asset Web

`asset-web` is the browser host for Asset Hub. It provides the authenticated asset workspace
(directory browsing, resource upload and management) and user administration, speaking only to
the core HTTP API.

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
| `npm test` | Run the test suite (currently empty; tests will be reintroduced) |
| `npm run build` | Create a production build |
| `npm run generate:api` | Regenerate HTTP-only OpenAPI declarations from a running API |
