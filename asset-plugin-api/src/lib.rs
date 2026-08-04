//! Asset Hub 插件协议。
//!
//! 这个 crate 是外部 Asset Hub 插件的 Rust SDK，只定义插件包和 Host/插件线协议。
//! 模块按职责边界拆分：
//!
//! - [`manifest`] 定义插件包的 Manifest 文档与校验。
//! - [`protocol`] 定义 Host 与插件 Action handler 之间的 JSON 线协议。
//! - [`abi`] 定义 Wasm 插件访问 Host 能力的版本化函数边界和 guest helper。
//!
//! 除 [`CRATE_VERSION`] 外，公共项只通过所属模块导出，使类型所有权保持明确。
//! Host 的归一化 Action/Kind、内置能力、执行策略和已加载资源不属于本 SDK。
//!
//! Cargo crate、Manifest 与统一 Plugin API 独立版本化；Plugin API 同时覆盖 Action
//! JSON、Wasm Host functions 和 Plugin Frame 消息。升级规则见
//! `asset-plugin-api/README.md`。

/// Cargo package version of the Rust authoring library. This is not a wire protocol version.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod abi;
pub mod manifest;
pub mod protocol;
