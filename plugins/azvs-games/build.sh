#!/usr/bin/env bash
set -euo pipefail

plugin_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repository_root="$(cd "$plugin_root/../.." && pwd)"
output="$plugin_root/asset-plugin-target"

if [[ "$output" != "$plugin_root/asset-plugin-target" ]]; then
  echo "refusing to clean an unexpected plugin output directory: $output" >&2
  exit 1
fi
rm -rf "$output"
mkdir -p "$output"

cargo build --locked --release --target wasm32-unknown-unknown \
  --manifest-path "$plugin_root/runtime/Cargo.toml"
install -m 0644 \
  "$plugin_root/runtime/target/wasm32-unknown-unknown/release/azvs_games_plugin.wasm" \
  "$output/plugin.wasm"

(cd "$repository_root/asset-plugin-sdk/web" && npm run build)
install -m 0644 \
  "$plugin_root/web/index.html" \
  "$plugin_root/web/app.js" \
  "$plugin_root/web/styles.css" \
  "$output/"
install -m 0644 \
  "$repository_root/asset-plugin-sdk/web/dist/asset-hub-plugin.global.js" \
  "$output/asset-hub-plugin.global.js"
install -m 0644 "$plugin_root/manifest.json" "$output/manifest.json"

echo "Games plugin artifacts: $output"
