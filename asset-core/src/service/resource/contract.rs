//! 资源服务的公开输入与输出契约。
//!
//! 本模块只描述调用方与资源应用服务交换的数据，不包含仓储或对象存储编排。

use crate::domain::{ResourceDirectory, ResourceId, ResourceKind, ResourceStatus};
use crate::port::BlobByteStream;
use asset_plugin_api::{PluginView, ResourceAction, ResourceActionDefinition};
use bytes::Bytes;

/// 创建不包含对象内容的资源。
#[derive(Debug, Clone)]
pub struct CreateResource {
    pub(super) name: String,
    pub(super) kind: Option<ResourceKind>,
    pub(super) status: ResourceStatus,
    pub(super) directory: ResourceDirectory,
    pub(super) description: Option<String>,
    pub(super) tags: Vec<String>,
}

impl CreateResource {
    /// 创建命令，默认使用 `core:file`、活跃状态、根目录、空描述和空标签。
    /// 名称在用例执行阶段原样校验，合法空格不会被裁剪。
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind: None,
            status: ResourceStatus::default(),
            directory: ResourceDirectory::root(),
            description: None,
            tags: Vec::new(),
        }
    }

    pub fn with_kind(mut self, kind: impl Into<ResourceKind>) -> Self {
        self.kind = Some(kind.into());
        self
    }

    pub fn with_status(mut self, status: ResourceStatus) -> Self {
        self.status = status;
        self
    }

    pub fn with_directory(mut self, directory: ResourceDirectory) -> Self {
        self.directory = directory;
        self
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_tags<T, I>(mut self, tags: I) -> Self
    where
        T: Into<String>,
        I: IntoIterator<Item = T>,
    {
        self.tags = tags.into_iter().map(Into::into).collect();
        self
    }

    pub fn directory(&self) -> &ResourceDirectory {
        &self.directory
    }
}

/// 创建带内容资源的通用命令。
///
/// 公共资源字段集中在这里，`payload` 表示不同写入用例特有的数据载荷。
#[derive(Debug, Clone)]
pub struct ResourceContentCommand<T> {
    pub(super) name: String,
    pub(super) kind: Option<ResourceKind>,
    pub(super) status: ResourceStatus,
    pub(super) directory: ResourceDirectory,
    pub(super) description: Option<String>,
    pub(super) tags: Vec<String>,
    pub(super) payload: T,
    pub(super) mime_type: Option<String>,
}

/// 流式上传内容并创建资源。
pub type UploadResourceContentStream = ResourceContentCommand<BlobByteStream>;

impl<T> ResourceContentCommand<T> {
    pub fn new(name: impl Into<String>, payload: T) -> Self {
        Self {
            name: name.into(),
            kind: None,
            status: ResourceStatus::default(),
            directory: ResourceDirectory::root(),
            description: None,
            tags: Vec::new(),
            payload,
            mime_type: None,
        }
    }

    pub fn with_kind(mut self, kind: impl Into<ResourceKind>) -> Self {
        self.kind = Some(kind.into());
        self
    }

    pub fn with_status(mut self, status: ResourceStatus) -> Self {
        self.status = status;
        self
    }

    pub fn with_directory(mut self, directory: ResourceDirectory) -> Self {
        self.directory = directory;
        self
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_tags<U, I>(mut self, tags: I) -> Self
    where
        U: Into<String>,
        I: IntoIterator<Item = U>,
    {
        self.tags = tags.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_mime_type(mut self, mime_type: impl Into<String>) -> Self {
        self.mime_type = Some(mime_type.into());
        self
    }

    pub fn directory(&self) -> &ResourceDirectory {
        &self.directory
    }
}

/// 执行资源动作。
#[derive(Debug, Clone)]
pub struct ExecuteResourceAction {
    pub(super) action: ResourceAction,
    pub(super) input: serde_json::Value,
}

impl ExecuteResourceAction {
    pub fn new(action: impl Into<ResourceAction>) -> Self {
        Self {
            action: action.into(),
            input: serde_json::Value::Object(Default::default()),
        }
    }

    pub fn with_input(mut self, input: serde_json::Value) -> Self {
        self.input = input;
        self
    }
}

/// 更新资源聚合。
#[derive(Debug, Clone, Default)]
pub struct UpdateResource {
    pub(super) name: Option<String>,
    pub(super) directory: Option<ResourceDirectory>,
    pub(super) kind: Option<ResourceKind>,
    pub(super) status: Option<ResourceStatus>,
    pub(super) description: Option<Option<String>>,
    pub(super) tags: Option<Vec<String>>,
    pub(super) restore: bool,
}

impl UpdateResource {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn with_directory(mut self, directory: ResourceDirectory) -> Self {
        self.directory = Some(directory);
        self
    }

    pub fn with_kind(mut self, kind: impl Into<ResourceKind>) -> Self {
        self.kind = Some(kind.into());
        self
    }

    pub fn with_status(mut self, status: ResourceStatus) -> Self {
        self.status = Some(status);
        self
    }

    pub fn with_description(mut self, description: Option<String>) -> Self {
        self.description = Some(description);
        self
    }

    pub fn with_tags<T, I>(mut self, tags: I) -> Self
    where
        T: Into<String>,
        I: IntoIterator<Item = T>,
    {
        self.tags = Some(tags.into_iter().map(Into::into).collect());
        self
    }

    pub fn with_restore(mut self, restore: bool) -> Self {
        self.restore = restore;
        self
    }

    pub fn directory(&self) -> Option<&ResourceDirectory> {
        self.directory.as_ref()
    }
}

/// 应用服务返回的可阅读资源结果。
#[derive(Debug, Clone, PartialEq)]
pub struct ReadableResource {
    id: ResourceId,
    name: String,
    kind: ResourceKind,
    view: PluginView,
}

impl ReadableResource {
    pub(super) fn new(id: ResourceId, name: String, kind: ResourceKind, view: PluginView) -> Self {
        Self {
            id,
            name,
            kind,
            view,
        }
    }

    pub fn id(&self) -> ResourceId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn kind(&self) -> &ResourceKind {
        &self.kind
    }

    pub fn view(&self) -> &PluginView {
        &self.view
    }
}

/// 资源当前可执行动作。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResourceActions {
    available_actions: Vec<ResourceActionDefinition>,
}

impl ResourceActions {
    pub(super) fn new(available_actions: Vec<ResourceActionDefinition>) -> Self {
        Self { available_actions }
    }

    pub fn available_actions(&self) -> &[ResourceActionDefinition] {
        &self.available_actions
    }
}

/// Core 内部使用的缓冲预览结果。
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ResourcePreview {
    content_type: String,
    content: Bytes,
}

#[cfg(test)]
impl ResourcePreview {
    pub(super) fn new(content_type: String, content: Bytes) -> Self {
        Self {
            content_type,
            content,
        }
    }

    pub(super) fn content_type(&self) -> &str {
        &self.content_type
    }

    pub(super) fn content(&self) -> &Bytes {
        &self.content
    }
}

/// 流式预览资源结果。
pub struct ResourcePreviewStream {
    content_type: String,
    content_length: Option<u64>,
    content: BlobByteStream,
}

impl ResourcePreviewStream {
    pub(super) fn new(
        content_type: String,
        content_length: Option<u64>,
        content: BlobByteStream,
    ) -> Self {
        Self {
            content_type,
            content_length,
            content,
        }
    }

    pub fn content_type(&self) -> &str {
        &self.content_type
    }

    pub fn content_length(&self) -> Option<u64> {
        self.content_length
    }

    pub fn into_content(self) -> BlobByteStream {
        self.content
    }
}

/// 流式读取资源内容结果。
pub struct ResourceContentStream {
    content_type: String,
    content_length: u64,
    content: BlobByteStream,
}

impl ResourceContentStream {
    pub(super) fn new(content_type: String, content_length: u64, content: BlobByteStream) -> Self {
        Self {
            content_type,
            content_length,
            content,
        }
    }

    pub fn content_type(&self) -> &str {
        &self.content_type
    }

    pub fn content_length(&self) -> u64 {
        self.content_length
    }

    pub fn into_content(self) -> BlobByteStream {
        self.content
    }
}

/// 缩略图结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceThumbnail {
    content_type: String,
    content: Bytes,
}

impl ResourceThumbnail {
    pub(super) fn new(content_type: String, content: Bytes) -> Self {
        Self {
            content_type,
            content,
        }
    }

    pub fn content_type(&self) -> &str {
        &self.content_type
    }

    pub fn content(&self) -> &Bytes {
        &self.content
    }
}
