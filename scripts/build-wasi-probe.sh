#!/usr/bin/env bash
# 构建真实 WASIp2 集成测试组件，不使用核心模块适配器。
set -euo pipefail
project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
if ! command -v rustup >/dev/null || ! command -v cargo >/dev/null || ! rustup target list --installed | grep -qx wasm32-wasip2; then
    echo "SKIP: cargo, rustup and the wasm32-wasip2 target are required" >&2
    exit 0
fi
CARGO_PROFILE_DEV_DEBUG=0 cargo build \
    --manifest-path "$project_dir/crates/vfs-plugin-wasm/tests/fixtures/wasi-probe/Cargo.toml" \
    --locked --target wasm32-wasip2 --target-dir "$project_dir/target/wasi-probe/guest" "$@"
echo "Built the WASIp2 test component under $project_dir/target/wasi-probe/guest"
