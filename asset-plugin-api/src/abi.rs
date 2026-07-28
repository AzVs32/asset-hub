//! Wasm 插件调用 Host function 的稳定 ABI 声明。
//!
//! ABI 与 JSON 协议独立版本化。各子模块定义函数名和输入边界，并可在
//! `extism-guest` 特性下提供 guest 侧调用封装；Host 端实现位于基础设施层。

pub mod content;
pub mod directory;

pub use content::{
    CONTENT_ABI_VERSION, CONTENT_CLOSE_FN, CONTENT_OPEN_FN, CONTENT_READ_RANGE_FN, CONTENT_SIZE_FN,
    ContentRangeError, PluginContentRange,
};
pub use directory::{
    DIRECTORY_HOST_API_VERSION, DIRECTORY_LIST_CHILDREN_FN, DIRECTORY_LIST_RESOURCES_FN,
};
