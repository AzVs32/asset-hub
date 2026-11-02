#!/usr/bin/env bash
# 构建真实 WASIp2 原型组件，不使用核心模块适配器。
set -euo pipefail
project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
if ! command -v rustup >/dev/null || ! command -v cargo >/dev/null || ! rustup target list --installed | grep -qx wasm32-wasip2; then
    echo "SKIP: cargo, rustup and the wasm32-wasip2 target are required" >&2
    exit 0
fi
CARGO_PROFILE_DEV_DEBUG=0 cargo build \
    --manifest-path "$project_dir/crates/vfs-plugin-wasm/tests/fixtures/wasi-prototype/Cargo.toml" \
    --locked --target wasm32-wasip2 --target-dir "$project_dir/target/wasi-prototype/guest" "$@"
echo "VFS_WASI_PROTOTYPE_COMPONENT=$project_dir/target/wasi-prototype/guest/wasm32-wasip2/debug/wasi_prototype.wasm"
# 运行原型并输出测量结果：
# VFS_WASI_PROTOTYPE_COMPONENT=<上面输出的路径> cargo test -p vfs-plugin-wasm --test wasi_prototype -- --nocapture
