//! Asset Hub 插件 SDK。
//!
//! 这个 crate 同时保存 Host/插件线协议和面向 Rust 插件作者的高层 API。
//! 模块按职责边界拆分：
//!
//! - [`manifest`] 定义插件包的 Manifest 文档与校验。
//! - [`protocol`] 定义 Host 与插件 Action handler 之间的 JSON 线协议。
//! - [`abi`] 定义 Wasm 插件访问 Host 能力的版本化函数边界和 guest helper。
//! - [`runtime`] 在 `extism-guest` 特性下实现 Action context、输出 builder 和有界 Host 访问。
//!
//! 宿主使用底层模块；插件业务代码可以直接从 crate 根导入高层作者 API。
//! Host 的归一化 Action/Kind、内置能力、执行策略和已加载资源不属于本 SDK。
//!
//! Cargo crate、Manifest 与统一 Plugin API 独立版本化；Plugin API 同时覆盖 Action
//! JSON、Wasm Host functions 和 Plugin Frame 消息。升级规则见
//! `asset-plugin-sdk/README.md`。

/// Cargo package version of the Rust authoring library. This is not a wire protocol version.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod abi;
pub mod manifest;
pub mod protocol;

#[cfg(feature = "extism-guest")]
pub mod runtime;

#[cfg(feature = "extism-guest")]
pub use runtime::{
    DirectoryChild, DirectoryContext, DirectoryResource, DirectoryResponse, DirectorySnapshot,
    Download, Error, Frame, Media, ResourceContent, ResourceContext, ResourceResponse,
    ResourceSnapshot, Result, Tree, View, decode_base64, encode_base64, encode_base64_url,
};
#[cfg(feature = "extism-guest")]
pub use serde::{Deserialize, Serialize};
#[cfg(feature = "extism-guest")]
pub use serde_json::{self, Value, json};

// Declarative export macros need a stable path when expanded in a plugin crate.
extern crate self as asset_plugin_sdk;

#[doc(hidden)]
#[cfg(feature = "extism-guest")]
pub mod __private {
    pub use extism_pdk::{self, FnResult, plugin_fn};
}

/// Exports a Resource Action while keeping Extism and wire serialization out of business code.
#[cfg(feature = "extism-guest")]
#[macro_export]
macro_rules! export_resource_action {
    ($export:ident => $handler:path) => {
        #[doc(hidden)]
        mod $export {
            use super::*;
            use ::asset_plugin_sdk::__private::extism_pdk;

            #[::asset_plugin_sdk::__private::plugin_fn]
            pub fn $export(input: String) -> ::asset_plugin_sdk::__private::FnResult<String> {
                ::asset_plugin_sdk::runtime::run_resource_action(input, $handler)
            }
        }

        pub use $export::$export;
    };
}

/// Exports a Directory Action while keeping Extism and wire serialization out of business code.
#[cfg(feature = "extism-guest")]
#[macro_export]
macro_rules! export_directory_action {
    ($export:ident => $handler:path) => {
        #[doc(hidden)]
        mod $export {
            use super::*;
            use ::asset_plugin_sdk::__private::extism_pdk;

            #[::asset_plugin_sdk::__private::plugin_fn]
            pub fn $export(input: String) -> ::asset_plugin_sdk::__private::FnResult<String> {
                ::asset_plugin_sdk::runtime::run_directory_action(input, $handler)
            }
        }

        pub use $export::$export;
    };
}
