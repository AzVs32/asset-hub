#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
target="wasm32-unknown-unknown"

build_and_verify() {
  local source_root="$1"
  local cargo_manifest="$2"
  local built_wasm="$3"

  cargo build --locked --release --target "$target" --manifest-path "$root/$cargo_manifest"
  cmp "$root/$built_wasm" "$root/$source_root/plugin.wasm"
}

build_and_verify \
  "plugins/azvs-markdown" \
  "plugins/azvs-markdown/runtime/Cargo.toml" \
  "plugins/azvs-markdown/runtime/target/$target/release/azvs_markdown_plugin.wasm"
build_and_verify \
  "plugins/azvs-epub" \
  "plugins/azvs-epub/runtime/Cargo.toml" \
  "plugins/azvs-epub/runtime/target/$target/release/azvs_epub_plugin.wasm"
