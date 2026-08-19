# Asset Hub

Asset Hub is a local-first asset management system with support for multiple user accounts. It
provides a Web interface and local administration commands, and can be extended with plugins.

## Getting Started

Install the Rust target and JavaScript dependencies required by the bundled plugins and Web app:

```bash
rustup target add wasm32-unknown-unknown
npm --prefix asset-plugin-sdk/web ci
npm --prefix plugins/resource-text/web ci
npm --prefix plugins/azvs-epub/web ci
npm --prefix plugins/directory-games/web ci
npm --prefix asset-web ci
```

### 1. Build and Install the Bundled Plugins

From the repository root, build the plugin packages:

```bash
plugins/resource-text/build.sh
plugins/resource-image/build.sh
plugins/azvs-epub/build.sh
plugins/directory-games/build.sh
```

Then install them into Asset Hub:

```bash
cargo run -p asset-cli --bin asset -- plugin --install plugins/resource-text/asset-plugin-target
cargo run -p asset-cli --bin asset -- plugin --install plugins/resource-image/asset-plugin-target
cargo run -p asset-cli --bin asset -- plugin --install plugins/azvs-epub/asset-plugin-target
cargo run -p asset-cli --bin asset -- plugin --install plugins/directory-games/asset-plugin-target
```

### 2. Create the First Administrator

```bash
cargo run -p asset-cli --bin asset -- user --create admin --admin
```

Enter the administrator password when prompted.

### 3. Start the API

```bash
cargo run -p asset-http --bin asset-http
```

The API listens on `http://127.0.0.1:8080` by default.

### 4. Start the Web Interface

In another terminal:

```bash
cd asset-web
npm run dev
```

Open `http://127.0.0.1:5173`.
