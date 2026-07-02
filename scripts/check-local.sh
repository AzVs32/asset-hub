#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
NODE_BIN="${NODE_BIN:-/storage/apps/node-v22.20.0/bin}"

if [[ -d "$NODE_BIN" ]]; then
  export PATH="$NODE_BIN:$PATH"
fi

if ! command -v node >/dev/null 2>&1; then
  echo "node was not found. Set NODE_BIN or add Node.js to PATH." >&2
  exit 1
fi

cd "$ROOT"
cargo test --workspace
cargo test --manifest-path plugins/azvs-epub/Cargo.toml
cargo test --manifest-path plugins/azvs-mp4/Cargo.toml

cd "$ROOT/asset-web-admin"
npm run lint
npm run build

cd "$ROOT/plugins/azvs-markdown"
npm test
