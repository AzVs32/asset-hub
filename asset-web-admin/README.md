# Asset Web Admin

Internal admin UI for Asset Hub.

Styling uses Tailwind CSS v4 through the official `@tailwindcss/vite` plugin.
Components use utility classes directly; `src/styles.css` only imports Tailwind
and defines the global font/body baseline.

## Development

Start the API server from the repository root:

```bash
cargo run -p asset-apps --bin asset-http
```

Install frontend dependencies and start Vite:

```bash
cd asset-web-admin
npm install
npm run dev
```

Vite serves the app on `http://127.0.0.1:5173` and proxies `/api` to
`http://127.0.0.1:8080`.

To call a different API origin, set:

```bash
VITE_API_BASE_URL=http://127.0.0.1:8080 npm run dev
```

## Checks

```bash
npm run lint
npm test
npm run build
```

## API types

HTTP request and response types are generated from the running API's OpenAPI document. Start
`asset-http` with Swagger enabled, then regenerate after an HTTP contract change:

```bash
npm run generate:api
```

Plugin view and iframe message types remain in `src/plugins/host` because the HTTP OpenAPI schema
intentionally exposes plugin views as JSON values. Individual plugins should use the declared
action locations and view contracts; they do not need imports from this application.

The host currently renders `resource_detail`, `context_menu`, and `resource_list_thumbnail`.
Actions that declare only unknown locations fall back to the resource detail action area, so an
unrecognized placement hint does not make a plugin action inaccessible. Plugin frames may request
any action currently present in the resource's `available_actions`; the host validates that list
before forwarding the request.
