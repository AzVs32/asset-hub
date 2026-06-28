# Asset Web Admin

Internal admin UI for Asset Hub.

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
npm run build
```
