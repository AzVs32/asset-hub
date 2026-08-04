//! Host 与插件 Action handler 之间的版本化 JSON 线协议。
//!
//! 协议层只定义可序列化的请求、输出、视图、副作用声明和诊断，不决定 Action 是否
//! 可用，也不执行插件声明的副作用。

/// Current and only supported Host/plugin wire and ABI version.
pub const PLUGIN_API_VERSION: &str = "asset-hub.plugin-api@1";

mod access;
pub mod diagnostic;
pub mod directory;
pub mod resource;
pub mod view;

pub use access::PluginActionAccess;
pub use diagnostic::{PluginActionFailure, PluginDiagnostic, PluginDiagnosticSeverity};
pub use directory::{
    CreateChildDirectoryEffect, DirectoryActionEffect, DirectoryPluginActionOutput,
    PluginDirectory, PluginDirectoryActionRequest, PluginDirectoryChild, PluginDirectoryPage,
    PluginDirectoryResource, PluginDirectoryResourcePage, UpdateDirectoryEffect,
};
pub use resource::{
    PluginActionRequest, PluginChecksum, PluginContentBytes, PluginContentReference,
    PluginContentReferenceEncoding, PluginContentVerificationStatus, PluginInlineContentEncoding,
    PluginResource, PluginResourceContent,
};
pub use view::{
    DownloadView, HtmlView, JsonView, MarkdownView, MediaView, PluginActionEffect,
    PluginActionOutput, PluginFrameView, PluginMediaEncoding, PluginReplacementEncoding,
    PluginView, ReplaceContentEffect, TextView,
};
