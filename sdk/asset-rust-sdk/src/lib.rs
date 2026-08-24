//! Asset Hub 的 Rust 插件作者 SDK。
//!
//!
//! 插件业务代码只从 crate root 导入高层 context、response builder、错误类型与导出宏。
//! 语言无关的 Manifest、protocol 和 ABI 契约由独立的 `asset-plugin-api` crate 持有。

/// Cargo package version of the Rust authoring library. This is not a wire protocol version.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(feature = "extism-guest")]
mod extism;
mod guest;
mod runtime;

pub use asset_plugin_api::protocol::PLUGIN_API_VERSION;
pub use runtime::{
    Diagnostic, DirectoryChild, DirectoryContext, DirectoryResource, DirectoryResponse,
    DirectorySnapshot, Download, Error, Frame, Media, ResourceContent, ResourceContext,
    ResourceResponse, ResourceSnapshot, Result, Tree, View, decode_base64, encode_base64,
    encode_base64_url,
};
pub use serde::{Deserialize, Serialize};
pub use serde_json::{self, Value, json};

// Declarative export macros need a stable path when expanded in a plugin crate.
extern crate self as asset_rust_sdk;

#[doc(hidden)]
#[cfg(feature = "extism-guest")]
pub mod __private {
    pub use crate::extism::{run_directory_action, run_resource_action};
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
            use ::asset_rust_sdk::__private::extism_pdk;

            #[::asset_rust_sdk::__private::plugin_fn]
            pub fn $export(input: String) -> ::asset_rust_sdk::__private::FnResult<String> {
                ::asset_rust_sdk::__private::run_resource_action(input, $handler)
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
            use ::asset_rust_sdk::__private::extism_pdk;

            #[::asset_rust_sdk::__private::plugin_fn]
            pub fn $export(input: String) -> ::asset_rust_sdk::__private::FnResult<String> {
                ::asset_rust_sdk::__private::run_directory_action(input, $handler)
            }
        }

        pub use $export::$export;
    };
}
