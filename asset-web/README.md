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

The only top-level Directory workspace handoff point is:

| Location | Host behavior |
| --- | --- |
| `directory_workspace` | A nearest-kind, read-only `workspace` Provider exclusively replaces the complete Core Directory content workspace with its `plugin_frame` |

The following locations are owned by `CoreDirectoryWorkspace`, not by the outer Host shell:

| Location | Host behavior |
| --- | --- |
| `directory_context_menu` | Entries in a Core directory-row context menu |
| `directory_thumbnail` | Core directory-row preview |
| `resource_context_menu` | Entries in a Core resource-row context menu |
| `resource_thumbnail` | Core resource-row preview |

Actions with no location, or only locations unknown to this host version, remain reachable through
`resource_context_menu` for resources and `directory_context_menu` for directories. Automatic
thumbnail slots deliberately ignore write actions. The detail panel's editing form hosts the
host-owned Save command. `core.resource.delete` and `core.directory.delete` are Host-owned ordinary
Actions discovered in the corresponding row menus; they carry destructive confirmation metadata,
return only a `delete` effect, and enter the existing authorized soft-delete/empty-directory-delete
use cases without returning a fake view. Restore remains a Host row-menu command for deleted
resources. Plugin actions are invoked from the corresponding row context menu. There are no
automatic plugin insertion points in the resource detail panel.

When a Directory kind resolves a `workspace` Provider, `CoreDirectoryWorkspace` is not mounted, so
none of its four internal locations exist. The plugin frame owns its complete internal UI and may
define private slots without registering them with the Host. The Host retains only its global
session shell, sandbox boundary, dialogs, confirmations, and navigation authority. Path breadcrumbs
share the primary header row with the Asset Hub title, while the current Directory kind editor sits
immediately above `directory_workspace`; both remain available for Core and plugin workspaces.
Changing a non-root Directory kind uses the normal
revision-guarded Directory update and causes the Host to resolve the workspace Provider again. The
root Directory kind remains immutable with the rest of the root aggregate metadata.

The backend resolves singleton capability providers before returning resource or directory
actions as flat arrays. Each kind/action includes its typed built-in or plugin origin. Actions use
`read` or `write` access and declare their possible `output.views` and `output.effects`. For example,
`azvs.epub.thumbnail` provides the Resource-scoped `thumbnail` capability
for EPUB resources.
`resource.image.thumbnail` provides the same capability on `core:resource` only when image MIME or
extension matching succeeds; it does not introduce an image Kind. Resources and directories with
no matching thumbnail provider use local File and Folder icon fallbacks without executing an
Action. Automatic thumbnail slots accept only the resolved `thumbnail` provider. Resource and
directory action registries scope that capability independently.

Supported output views are `text`, `markdown`, `html`, `plugin_frame`, `json`, `media`, and
`download`. Generic outputs are rendered by the host. A plugin
that needs its own application UI returns `plugin_frame` with a verified `/plugins/<id>/...` path;
the frame runs with `sandbox="allow-scripts"` and can request only actions already exposed for the
current Resource or Directory through the versioned Asset Hub Web Plugin SDK. The SDK hides its Penpal transport
and is available as both an ESM package and a self-contained script for plain `index.html` plugins.
A frame produced by the current
write `text_edit` provider may also request raw text replacement; plugin Manifest validation
requires that provider to request `resource.content.replace`. The Host binds it to that resource
and sends the content through the Host's revision-guarded streaming replacement use case.
A frame may invoke only Actions exposed for its bound Resource. Destructive Actions,
including deletion, require a Host confirmation before the Gateway call is made.
Directory frames use a separate Directory-bound bridge to execute exposed Directory Actions,
refresh the current Directory, and request canonical Host navigation. They cannot access Core
workspace slots or address arbitrary Directory IDs.

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
