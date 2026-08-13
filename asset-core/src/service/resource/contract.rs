//! 资源服务的公开输入与输出契约。
//!
//! 本模块只描述调用方与资源应用服务交换的数据，不包含仓储或对象存储编排。

use crate::domain::{Checksum, DirectoryPath, ResourceId, ResourceKind};
use crate::domain::{ResourceActionDefinition, ResourceActionId};
use crate::port::BlobByteStream;

/// 创建持久化上传会话。
#[derive(Debug, Clone)]
pub struct CreateUpload {
    pub(super) name: String,
    pub(super) kind: Option<ResourceKind>,
    pub(super) directory: DirectoryPath,
    pub(super) mime_type: Option<String>,
    pub(super) expected_size: u64,
    pub(super) expected_checksum: Checksum,
}

impl CreateUpload {
    pub fn new(name: impl Into<String>, expected_size: u64, expected_checksum: Checksum) -> Self {
        Self {
            name: name.into(),
            kind: None,
            directory: DirectoryPath::root(),
            mime_type: None,
            expected_size,
            expected_checksum,
        }
    }

    pub fn with_kind(mut self, kind: ResourceKind) -> Self {
        self.kind = Some(kind);
        self
    }

    pub fn with_directory(mut self, directory: DirectoryPath) -> Self {
        self.directory = directory;
        self
    }

    pub fn with_mime_type(mut self, mime_type: impl Into<String>) -> Self {
        self.mime_type = Some(mime_type.into());
        self
    }

    pub fn directory(&self) -> &DirectoryPath {
        &self.directory
    }
}

/// 执行资源动作。
#[derive(Debug, Clone)]
pub struct ExecuteResourceAction {
    pub(super) action: ResourceActionId,
    pub(super) input: serde_json::Value,
    pub(super) expected_revision: Option<u64>,
}

/// 流式替换可编辑文本资源的内容。
#[derive(Debug, Clone)]
pub struct ReplaceResourceContent {
    pub(super) expected_size: u64,
    pub(super) expected_checksum: Checksum,
    pub(super) expected_revision: u64,
    pub(super) mime_type: Option<String>,
}

impl ReplaceResourceContent {
    pub fn new(expected_size: u64, expected_checksum: Checksum, expected_revision: u64) -> Self {
        Self {
            expected_size,
            expected_checksum,
            expected_revision,
            mime_type: None,
        }
    }

    pub fn with_mime_type(mut self, mime_type: impl Into<String>) -> Self {
        self.mime_type = Some(mime_type.into());
        self
    }
}

impl ExecuteResourceAction {
    pub fn new(action: ResourceActionId, expected_revision: Option<u64>) -> Self {
        Self {
            action,
            input: serde_json::Value::Object(Default::default()),
            expected_revision,
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
    pub(super) expected_revision: u64,
    pub(super) name: Option<String>,
    pub(super) directory: Option<DirectoryPath>,
    pub(super) kind: Option<ResourceKind>,
    pub(super) restore: bool,
}

impl UpdateResource {
    pub fn new(expected_revision: u64) -> Self {
        Self {
            expected_revision,
            ..Self::default()
        }
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn with_directory(mut self, directory: DirectoryPath) -> Self {
        self.directory = Some(directory);
        self
    }

    pub fn with_kind(mut self, kind: ResourceKind) -> Self {
        self.kind = Some(kind);
        self
    }

    pub fn with_restore(mut self, restore: bool) -> Self {
        self.restore = restore;
        self
    }

    pub fn directory(&self) -> Option<&DirectoryPath> {
        self.directory.as_ref()
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

/// Authorized, point-in-time directory tree projection used to build a ZIP download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryArchiveManifest {
    filename: String,
    directories: Vec<String>,
    resources: Vec<DirectoryArchiveResource>,
}

impl DirectoryArchiveManifest {
    pub(super) fn new(
        filename: String,
        directories: Vec<String>,
        resources: Vec<DirectoryArchiveResource>,
    ) -> Self {
        Self {
            filename,
            directories,
            resources,
        }
    }

    pub fn filename(&self) -> &str {
        &self.filename
    }

    pub fn directories(&self) -> &[String] {
        &self.directories
    }

    pub fn resources(&self) -> &[DirectoryArchiveResource] {
        &self.resources
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryArchiveResource {
    resource_id: ResourceId,
    path: String,
    content_length: u64,
}

impl DirectoryArchiveResource {
    pub(super) fn new(resource_id: ResourceId, path: String, content_length: u64) -> Self {
        Self {
            resource_id,
            path,
            content_length,
        }
    }

    pub fn resource_id(&self) -> ResourceId {
        self.resource_id
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn content_length(&self) -> u64 {
        self.content_length
    }
}
