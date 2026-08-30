# Asset Web

`asset-web` is the browser host for Asset Hub. It provides the authenticated asset workspace,
user administration, and the secure UI boundary for backend plugin actions and views.

For dependency rules, state ownership, runtime flows, and plugin-host constraints, see
[`ARCHITECTURE.md`](ARCHITECTURE.md). Plugin authors should use the
[`asset-web-sdk`](../sdk/asset-web-sdk/README.md).

## Prerequisites

- Node.js 22.12 or later.
- `asset-http` running on `http://127.0.0.1:8080`.

## Quick start

```bash
npm ci
npm run dev
```

The development server listens on `http://127.0.0.1:5173` and connects to the local API.

To use a different API origin:

```bash
VITE_API_BASE_URL=http://127.0.0.1:8080 npm run dev
```

## Commands

| Command | Purpose |
| --- | --- |
| `npm run dev` | Start the local development server |
| `npm run check` | Run formatting/lint checks and TypeScript validation |
| `npm test` | Run the focused executable-contract tests |
| `npm run build` | Create a production build |
| `npm run generate:api` | Regenerate HTTP-only OpenAPI declarations from a running API |

## Plugin development

The API snapshots verified plugin files at startup. Restart `asset-http` after changing a plugin
package. Changes that stay within an existing host slot, capability, and view kind do not require a
frontend rebuild.
