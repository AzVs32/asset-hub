//! Asset Hub 插件协议。
//!
//! 这个 crate 定义 Asset Hub、插件 manifest 和插件 action handler 之间共享的协议。
//! 模块按协议层次拆分：
//!
//! - [`manifest`] 对应插件 manifest JSON 文档。
//!   - `plugin` 描述插件自身的注册信息。
//!   - `runtime` 描述插件的执行方式。
//!   - `capabilities` 描述插件向 host 提供的能力。
//!   - `permissions` 描述插件运行时需要的权限边界。
//! - [`action`] 定义 Asset Hub 归一化后的资源 action 模型。manifest 加载后，
//!   host 使用这些类型在不同资源上列出、匹配和执行 action；manifest 中声明的
//!   action capability 会先转换成这里的 action definition 再进入运行时。
//! - [`request`] 定义 host 发送给插件 action handler 的 JSON 输入协议，包括资源快照、
//!   可选的 inline content，以及 host 可读取的 content reference。
//! - [`view`] 定义插件 action handler 返回给 host 的 JSON 输出协议，用于表达文本、
//!   HTML、媒体、表格、表单、二进制 URL 等可由 host 渲染的视图。
//!
//! Cargo crate、Manifest、JSON plugin API 和 Wasm content ABI 独立版本化；升级规则见
//! `asset-plugin-api/README.md`。

/// Cargo package version of the Rust authoring library. This is not a wire protocol version.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Browser assets loaded from verified plugin packages, grouped by plugin id
/// and package-relative path.
pub type PluginWebAssets = std::collections::HashMap<
    String,
    std::collections::HashMap<std::path::PathBuf, std::sync::Arc<[u8]>>,
>;

pub mod action;
pub mod content;
pub mod diagnostic;
pub mod manifest;
pub mod policy;
pub mod request;
pub mod view;

pub use action::{
    ResourceAction, ResourceActionAccess, ResourceActionAppliesTo, ResourceActionContentDelivery,
    ResourceActionDefinition, ResourceActionExecutorKind, ResourceActionOutputContract,
    ResourceActionRequirements, ResourceActionUi, ResourceContentMatcher,
};
pub use content::{
    CONTENT_ABI_VERSION, CONTENT_CLOSE_FN, CONTENT_OPEN_FN, CONTENT_READ_RANGE_FN, CONTENT_SIZE_FN,
    ContentRangeError, PluginContentRange,
};
pub use diagnostic::{PluginActionFailure, PluginDiagnostic, PluginDiagnosticSeverity};
pub use manifest::{
    ActionAppliesTo, ActionRequirements, ActionUi, ContentDelivery, DirectoryKindCapability,
    FilesystemPermission, MANIFEST_VERSION, MIN_MANIFEST_VERSION, ManifestActionAccess,
    NetworkPermission, PLUGIN_API_VERSION, PluginCapabilities, PluginDescriptor, PluginManifest,
    PluginManifestLock, PluginPermission, PluginPermissions, PluginRuntime, PluginRuntimeLock,
    PluginWeb, PluginWebLock, ReadWritePermission, ResourceActionCapability,
    ResourceKindCapability, is_plugin_api_compatible,
};
pub use policy::{InvalidPluginExecutionPolicy, PluginExecutionPolicy};
pub use request::{
    PluginActionRequest, PluginChecksum, PluginContentBytes, PluginContentReference,
    PluginContentReferenceEncoding, PluginInlineContentEncoding, PluginResource,
    PluginResourceContent,
};
pub use view::{
    BinaryUrlView, FormView, HtmlView, JsonView, MarkdownView, MediaView, PluginActionEffect,
    PluginActionOutput, PluginFrameView, PluginMediaEncoding, PluginReplacementEncoding,
    PluginView, ReplaceContentEffect, TableColumn, TableView, TextView,
};
