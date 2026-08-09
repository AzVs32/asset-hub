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
├── plugins/                generic action, slot, iframe, and view hosting
├── features/               resource workspace, authentication, user management
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
Action discovery exposes matching, access, output, and UI metadata only; executor selection and
handler bindings remain private to the backend Host.

Stable slots:

| Location | Host behavior |
| --- | --- |
| `resource_detail` | User-triggered buttons in the detail action bar |
| `context_menu` | User-triggered resource row menu items |
| `resource_list_thumbnail` | Automatically executes a read-only action for list preview |
| `directory_list_thumbnail` | Automatically executes a read-only action for directory preview |
| `resource_detail_panel` | Automatically renders read-only output below resource facts |
| `resource_detail_aside` | Automatically renders read-only output above the core editor |

Actions with no location, or only locations unknown to this host version, remain reachable through
`resource_detail`. Automatic slots deliberately ignore write actions.

The backend resolves singleton capability providers before returning resource or directory
actions as flat arrays. Each kind/action includes its typed built-in or plugin origin, and actions
use `read` or `write` access plus an `output.views` contract. For example,
`azvs.epub.thumbnail` provides the Resource-scoped `thumbnail` capability
for EPUB resources.
`core:image` similarly resolves to the Host-owned `core.image.thumbnail`; other resource kinds
retain the kind-neutral generic provider. Automatic thumbnail slots accept only the corresponding
resolved `thumbnail` provider. Resource and directory action registries scope that capability
independently.

Supported output views are `text`, `markdown`, `html`, `plugin_frame`, `json`, `media`, and
`download`. Generic outputs are rendered by the host. A plugin
that needs its own application UI returns `plugin_frame` with a verified `/plugins/<id>/...` path;
the frame runs with `sandbox="allow-scripts"` and can request only actions already exposed for the
current resource through the versioned Asset Hub Web Plugin SDK. The SDK hides its Penpal transport
and is available as both an ESM package and a self-contained script for plain `index.html` plugins.
A frame produced by the current
write `text_edit` provider may also request raw text replacement; the Host binds it to that
resource and sends the content through the same revision-guarded streaming use case as the core
text editor.

Directories are addressed by stable UUID throughout the domain and Gateway. Paths are navigation
labels only. Directory update/delete and Resource or Directory actions forward the aggregate's
current revision whenever they can write. Read actions operate on the latest authorized
snapshot without producing avoidable stale-preview conflicts. A coded write conflict invalidates
the relevant queries and tells the user that the latest version has been loaded.

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
