# Asset Web

`asset-web` is the React host for Asset Hub. Its architecture mirrors the Rust workspace: domain
types and application ports are kept independent from HTTP and React, while plugin-specific
behavior crosses one small kernel boundary.

中文架构说明见 [`ARCHITECTURE.md`](ARCHITECTURE.md)。

## Architecture

```text
src/
├── domain/                 resource, authentication, plugin view contracts
├── application/
│   ├── ports/              AssetGateway and React composition boundary
│   └── queries/            stable cache keys
├── infrastructure/http/    OpenAPI transport and DTO ↔ domain mapping
├── kernel/                 host slots and plugin view renderer registry
├── plugins/                generic action, slot, iframe, form, and view hosting
├── features/               resource workspace, authentication, user access
├── shared/ui/              small Radix-backed host design system
└── app/                    composition root and routing
```

The dependency direction is inward: features use the `AssetGateway` port and domain types; only
the HTTP adapter knows snake_case OpenAPI DTOs. Plugin JSON is validated with Zod before it reaches
a renderer. Server state is owned by TanStack Query, forms by React Hook Form, accessible overlay
behavior by Radix, and formatting/linting by Biome.

## Plugin Host Contract

A backend plugin contributes actions through its manifest. The host discovers the available
actions from each resource response, so adding or changing a plugin does not require editing or
rebuilding `asset-web` when it uses an existing slot and view kind.

Stable slots:

| Location | Host behavior |
| --- | --- |
| `resource_detail` | User-triggered buttons in the detail action bar |
| `context_menu` | User-triggered resource row menu items |
| `resource_list_thumbnail` | Automatically executes a read-only action for list preview |
| `resource_detail_panel` | Automatically renders read-only output below resource facts |
| `resource_detail_aside` | Automatically renders read-only output above the core editor |

Actions with no location, or only locations unknown to this host version, remain reachable through
`resource_detail`. Automatic slots deliberately ignore write actions.

Supported output views are `text`, `markdown`, `html`, `plugin_frame`, `json`, `media`,
`binary_url`, `table`, and JSON Schema `form`. Generic outputs are rendered by the host. A plugin
that needs its own application UI returns `plugin_frame` with a verified `/plugins/<id>/...` path;
the frame runs with `sandbox="allow-scripts"` and can request only actions already exposed for the
current resource through the versioned `postMessage` protocol.

Adding a new slot or a new output view kind is a host protocol change and therefore does require a
frontend update. Adding a plugin that consumes the contract does not. The API currently snapshots
verified plugin files at startup, so restart the API after changing a plugin package; the frontend
does not need to be changed.

## Development

Start `asset-http` on port 8080, then:

```bash
cd asset-web
npm ci
npm run dev
```

Vite serves `http://127.0.0.1:5173` and proxies `/api` to the API. To use another API origin:

```bash
VITE_API_BASE_URL=http://127.0.0.1:8080 npm run dev
```

Regenerate the transport-only OpenAPI declarations while the API is running:

```bash
npm run generate:api
```

## Verification

```bash
npm run check
npm test
npm run build
```
