//! Plugin API 向 Wasm 插件暴露的 Host function 声明。
//!
//! Host function 与 Action JSON、Plugin Frame 共同由
//! [`crate::protocol::PLUGIN_API_VERSION`] 版本化。
//! 各子模块只定义运行时无关的函数名、输入边界和值对象；具体 guest 调用适配器由各语言
//! SDK 实现，Host 端实现位于基础设施层。

pub mod content;
pub mod directory;

pub use content::{
    CONTENT_CLOSE_FN, CONTENT_OPEN_FN, CONTENT_READ_RANGE_FN, CONTENT_SIZE_FN, ContentRangeError,
    PluginContentRange,
};
pub use directory::{
    DIRECTORY_LIST_CHILDREN_FN, DIRECTORY_LIST_RESOURCES_FN, DIRECTORY_PAGE_MAX_LIMIT,
    PluginDirectoryPageRequest,
};
