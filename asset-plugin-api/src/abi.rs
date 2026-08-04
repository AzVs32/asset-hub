//! Plugin API 向 Wasm 插件暴露的 Host function 声明。
//!
//! Host function 与 Action JSON、Plugin Frame 共同由
//! [`crate::protocol::PLUGIN_API_VERSION`] 版本化。
//! 各子模块定义函数名和输入边界，并可在 `extism-guest` 特性下提供 guest 侧调用
//! 封装；Host 端实现位于基础设施层。

pub mod content;
pub mod directory;

pub use content::{
    CONTENT_CLOSE_FN, CONTENT_OPEN_FN, CONTENT_READ_RANGE_FN, CONTENT_SIZE_FN, ContentRangeError,
    PluginContentRange,
};
pub use directory::{DIRECTORY_LIST_CHILDREN_FN, DIRECTORY_LIST_RESOURCES_FN};
