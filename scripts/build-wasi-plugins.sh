#!/usr/bin/env bash
# 构建直接产出组件的 WASIp2 插件；额外参数例如 --offline、--release 原样传给 Cargo。
set -euo pipefail
project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
if ! command -v cargo >/dev/null || ! command -v rustup >/dev/null || ! rustup target list --installed | grep -qx wasm32-wasip2; then
    echo "SKIP: cargo, rustup and the wasm32-wasip2 target are required" >&2
    exit 0
fi
for plugin in s3 webdav; do
    CARGO_PROFILE_DEV_DEBUG=0 cargo build --locked \
        --manifest-path "$project_dir/plugins/vfs-driver-$plugin/Cargo.toml" \
        --target wasm32-wasip2 --target-dir "$project_dir/target/wasi-plugins" "$@"
done
