#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
target="wasm32-unknown-unknown"

build_and_verify() {
  local manifest_path="$1"
  local cargo_manifest="$2"
  local built_wasm="$3"
  local deployed_wasm="$4"

  cargo build --locked --release --target "$target" --manifest-path "$root/$cargo_manifest"
  cmp "$root/$built_wasm" "$root/$deployed_wasm"

  cargo run --manifest-path "$root/Cargo.toml" --locked --quiet \
    -p asset-apps --bin asset-plugin -- \
    verify-wasm "$root/$manifest_path"
}

build_and_verify \
  "plugins/azvs-markdown/azvs-markdown.json" \
  "plugins/azvs-markdown/plugin/Cargo.toml" \
  "plugins/azvs-markdown/plugin/target/$target/release/azvs_markdown_plugin.wasm" \
  "plugins/azvs-markdown/azvs-markdown.wasm"
build_and_verify \
  "plugins/azvs-epub/manifest.json" \
  "plugins/azvs-epub/runtime/Cargo.toml" \
  "plugins/azvs-epub/runtime/target/$target/release/azvs_epub_plugin.wasm" \
  "plugins/azvs-epub/azvs-epub.wasm"
build_and_verify \
  "plugins/azvs-mp4/azvs-mp4.json" \
  "plugins/azvs-mp4/Cargo.toml" \
  "plugins/azvs-mp4/target/$target/release/azvs_mp4_plugin.wasm" \
  "plugins/azvs-mp4/azvs-mp4.wasm"
