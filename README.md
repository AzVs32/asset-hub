# Asset Hub

Asset Hub is a local-first asset management system with multiple user accounts, a Web interface,
and local administration commands.

## Getting started

Create the first administrator. The command prompts twice for a password:

```bash
cargo run -p asset-cli --bin asset -- user --create admin --admin
```

Install the Web application's dependencies:

```bash
npm --prefix asset-web ci
```

Start the API:

```bash
cargo run -p asset-http --bin asset-http
```

It listens on `http://127.0.0.1:8080` by default. In another terminal, start the Web interface:

```bash
cd asset-web
npm run dev
```

Open `http://127.0.0.1:5173`.
