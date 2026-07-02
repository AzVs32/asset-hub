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

pub mod action;
pub mod manifest;
pub mod request;
pub mod view;

pub use action::{
    ResourceAction, ResourceActionAccess, ResourceActionContentDelivery, ResourceActionDefinition,
    ResourceActionWhen,
};
pub use manifest::{
    ActionAppliesTo, ActionExecutor, ActionOutputContract, ActionRequirements, ActionUi,
    ContentDelivery, FilesystemPermission, MANIFEST_VERSION, ManifestActionAccess,
    NetworkPermission, PLUGIN_API_VERSION, PluginCapabilities, PluginManifest, PluginMetadata,
    PluginPermissions, PluginRuntime, ReadWritePermission, ResourceActionCapability,
    ResourceKindCapability,
};
pub use request::{
    PluginActionRequest, PluginChecksum, PluginContentBytes, PluginContentReference,
    PluginResource, PluginResourceContent,
};
pub use view::{
    BinaryUrlView, FormView, HtmlView, JsonView, MarkdownView, MediaView, PluginActionOutput,
    PluginContentEncoding, PluginView, TableColumn, TableView, TextView,
};
