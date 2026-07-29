//! Asset Hub 插件协议。
//!
//! 这个 crate 定义 Asset Hub、插件 manifest 和插件 action handler 之间共享的模型与协议。
//! 模块按职责边界拆分：
//!
//! - [`domain`] 定义 Manifest 归一化后供 Host 注册和匹配的 Action 领域模型。
//! - [`manifest`] 定义插件包的 Manifest 文档、校验和领域归一化入口。
//! - [`protocol`] 定义 Host 与插件 Action handler 之间的 JSON 线协议。
//! - [`abi`] 定义 Wasm 插件访问 Host 能力的版本化函数边界和 guest helper。
//! - [`policy`] 定义 Host 创建插件执行环境时使用的资源限制策略。
//!
//! 根部的 `action`、`request`、`view`、`diagnostic` 和 `content` 模块名作为兼容入口
//! 继续导出；Directory 协议与 ABI 分别通过 [`protocol::directory`] 和
//! [`abi::directory`] 访问。
//!
//! Cargo crate、Manifest 与统一 Plugin API 独立版本化；Plugin API 同时覆盖 Action
//! JSON、Wasm Host functions 和 Plugin Frame 消息。升级规则见
//! `asset-plugin-api/README.md`。

/// Cargo package version of the Rust authoring library. This is not a wire protocol version.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod abi;
pub mod domain;
pub mod manifest;
pub mod policy;
pub mod protocol;

pub use abi::content;
pub use domain::action;
pub use protocol::resource as request;
pub use protocol::{diagnostic, view};

pub use action::{
    ActionAccess, ActionDefinition, ActionExecutorKind, ActionId, ActionOutputContract,
    ActionUi as ActionDefinitionUi, DirectoryAction, DirectoryActionAccess,
    DirectoryActionAppliesTo, DirectoryActionDefinition, DirectoryActionExecutorKind,
    DirectoryActionOutputContract, DirectoryActionRequirements, DirectoryActionUi, ResourceAction,
    ResourceActionAccess, ResourceActionAppliesTo, ResourceActionContentDelivery,
    ResourceActionDefinition, ResourceActionExecutorKind, ResourceActionOutputContract,
    ResourceActionRequirements, ResourceActionUi, ResourceContentMatcher,
};
pub use content::{
    CONTENT_CLOSE_FN, CONTENT_OPEN_FN, CONTENT_READ_RANGE_FN, CONTENT_SIZE_FN, ContentRangeError,
    PluginContentRange,
};
pub use diagnostic::{PluginActionFailure, PluginDiagnostic, PluginDiagnosticSeverity};
pub use manifest::{
    ActionAppliesTo, ActionRequirements, ActionUi, ContentDelivery,
    DirectoryActionAppliesToCapability, DirectoryActionCapability,
    DirectoryActionRequirementsCapability, DirectoryKindCapability, FilesystemPermission,
    MANIFEST_VERSION, ManifestActionAccess, NetworkPermission, PLUGIN_API_VERSION,
    PLUGIN_LOCK_FILE_NAME, PLUGIN_MANIFEST_FILE_NAME, PLUGIN_WASM_FILE_NAME,
    PLUGIN_WEB_ENTRY_FILE_NAME, PluginCapabilities, PluginDescriptor, PluginManifest,
    PluginManifestLock, PluginPermission, PluginPermissions, PluginRuntime, PluginWebAssets,
    ResourceActionCapability, ResourceKindCapability,
};
pub use policy::{InvalidPluginExecutionPolicy, PluginExecutionPolicy};
pub use request::{
    PluginActionRequest, PluginChecksum, PluginContentBytes, PluginContentReference,
    PluginContentReferenceEncoding, PluginContentVerificationStatus, PluginInlineContentEncoding,
    PluginResource, PluginResourceContent,
};
pub use view::{
    DownloadView, HtmlView, JsonView, MarkdownView, MediaView, PluginActionEffect,
    PluginActionOutput, PluginFrameView, PluginMediaEncoding, PluginReplacementEncoding,
    PluginView, ReplaceContentEffect, TextView,
};
