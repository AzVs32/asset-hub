//! Host 与插件 Action handler 之间的版本化 JSON 线协议。
//!
//! 协议层只定义可序列化的请求、输出、视图、副作用声明和诊断，不决定 Action 是否
//! 可用，也不执行插件声明的副作用。

/// Current and only supported Host/plugin wire and ABI version.
pub const PLUGIN_API_VERSION: &str = "asset-hub.plugin-api@4";

/// Browser Frame channel used for Resource-bound Host capabilities.
pub const RESOURCE_FRAME_CHANNEL: &str = "asset-hub.plugin-frame.resource@4";

/// Browser Frame channel used for Directory-bound Host capabilities.
pub const DIRECTORY_FRAME_CHANNEL: &str = "asset-hub.plugin-frame.directory@4";

/// View discriminants supported by the current action and Browser Frame protocol.
pub const VIEW_KINDS: &[&str] = &[
    "text",
    "markdown",
    "html",
    "plugin_frame",
    "json",
    "media",
    "download",
];

/// Resource effect discriminants exposed to Browser Frame clients after Host application.
pub const RESOURCE_ACTION_EFFECT_KINDS: &[&str] = &["replace_content", "delete"];

/// Directory effect discriminants exposed to Browser Frame clients after Host application.
pub const DIRECTORY_ACTION_EFFECT_KINDS: &[&str] =
    &["update", "create_child", "create_tree", "delete"];

mod access;
mod diagnostic;
mod directory;
mod frame;
mod resource;
mod view;

pub use access::PluginActionAccess;
pub use diagnostic::codes as diagnostic_codes;
pub use diagnostic::{PluginActionFailure, PluginDiagnostic, PluginDiagnosticSeverity};
pub use directory::{
    CreateChildDirectoryEffect, CreateDirectoryTreeEffect, CreateTreeDirectory, CreateTreeResource,
    CreateTreeResourceEncoding, DirectoryActionEffect, PluginDirectory,
    PluginDirectoryActionOutput, PluginDirectoryActionRequest, PluginDirectoryActionResult,
    PluginDirectoryChild, PluginDirectoryPage, PluginDirectoryResource,
    PluginDirectoryResourcePage, UpdateDirectoryEffect,
};
pub use frame::{
    DIRECTORY_FRAME_METHODS, DirectoryFrameActionOutput, DirectoryFrameEffectKind,
    FrameArgumentSpec, FrameMethodSpec, RESOURCE_FRAME_METHODS, ResourceFrameActionOutput,
    ResourceFrameEffectKind,
};
pub use resource::{
    PluginChecksum, PluginContentBytes, PluginContentReference, PluginContentReferenceEncoding,
    PluginInlineContentEncoding, PluginReplacementEncoding, PluginResource,
    PluginResourceActionEffect, PluginResourceActionOutput, PluginResourceActionRequest,
    PluginResourceActionResult, PluginResourceContent, PluginResourceContentState,
    PluginResourceEffectiveState, PluginResourceLifecycleState, PluginResourceState,
    ReplaceContentEffect,
};
pub use view::{
    DownloadView, HtmlView, JsonView, MarkdownView, MediaView, PluginFrameView,
    PluginMediaEncoding, PluginView, TextView,
};
