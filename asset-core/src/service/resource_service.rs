//! 资源应用服务。
//!
//! 本模块提供围绕 `Resource` 聚合的应用用例。每个公开方法代表一个完整业务动作，
//! 负责把领域模型、资源仓储端口和对象存储端口编排在一起。
//!
//! 该层只定义业务流程，不绑定具体基础设施。OpenDAL、sqlx 等实现应通过 `port`
//! 模块中的 trait 注入进来。

use crate::CoreError;
use crate::domain::{
    Checksum, ChecksumKind, Resource, ResourceContent, ResourceId, ResourceKind, ResourceMetadata,
    ResourceStatus, StorageKey,
};
use crate::port::{
    BlobByteStream, BlobStorage, ListResources, ResourceActionExecutor, ResourceActionOutput,
    ResourceActionRegistry, ResourceActionRequest, ResourceKindRegistry, ResourcePage,
    ResourceRepository,
};
use asset_plugin_api::{PluginActionEffect, PluginContentEncoding, PluginView};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use bytes::Bytes;
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::sync::Arc;

const MAX_INLINE_PLUGIN_CONTENT_BYTES: u64 = 4 * 1024 * 1024;

/// 创建纯元数据资源的用例命令。
///
/// 该命令描述“创建一条没有对象内容的资源”的输入参数。它只收集调用方传入的数据，
/// 不直接访问数据库或对象存储。
///
/// 字段校验发生在 `ResourceService::create_resource` 执行时：资源名称、类型等会通过
/// 领域模型统一校验，校验失败会返回 `CoreError::Resource`。
#[derive(Debug, Clone)]
pub struct CreateResource {
    /// 资源展示名。
    name: String,
    /// 资源类型；未设置时会按内容特征自动推断，推断失败时使用 `core:file`。
    kind: Option<ResourceKind>,
    /// 初始生命周期状态。
    status: ResourceStatus,
    /// 初始资源元数据。
    metadata: ResourceMetadata,
}

impl CreateResource {
    /// 创建命令，默认自动推断资源类型、使用活跃状态和空元数据。
    ///
    /// `name` 会在 usecase 执行时去除首尾空白并校验，不会在命令构造阶段提前校验。
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind: None,
            status: ResourceStatus::default(),
            metadata: ResourceMetadata::default(),
        }
    }

    /// 设置资源类型。
    ///
    /// 未调用该方法时，资源类型使用 `core:file`。传入字符串时会在 usecase
    /// 执行阶段转换并校验。
    pub fn with_kind(mut self, kind: impl Into<ResourceKind>) -> Self {
        self.kind = Some(kind.into());
        self
    }

    /// 设置初始生命周期状态。
    ///
    /// 未调用该方法时，资源状态默认为 `ResourceStatus::Active`。
    pub fn with_status(mut self, status: ResourceStatus) -> Self {
        self.status = status;
        self
    }

    /// 设置初始资源元数据。
    ///
    /// 未调用该方法时，资源元数据默认为服务端定义的空元数据结构。
    pub fn with_metadata(mut self, metadata: impl Into<ResourceMetadata>) -> Self {
        self.metadata = metadata.into();
        self
    }
}

/// 上传内容并创建资源的用例命令。
///
/// 该命令描述“写入对象内容并创建资源记录”的输入参数。执行时会先构建
/// `ResourceContent`，再写入 `BlobStorage`，最后通过 `ResourceRepository` 保存资源聚合。
///
/// `storage_key` 必须由 `StorageKey` 构造，确保对象键已经通过领域规则校验。内容大小由
/// `data.len()` 自动计算，调用方不需要单独传入。
#[derive(Debug, Clone)]
pub struct UploadResourceContent {
    /// 资源展示名。
    name: String,
    /// 资源类型；未设置时使用 `core:file`。
    kind: Option<ResourceKind>,
    /// 初始生命周期状态。
    status: ResourceStatus,
    /// 初始资源元数据。
    metadata: ResourceMetadata,
    /// 内容在对象存储中的定位键。
    storage_key: StorageKey,
    /// 需要写入对象存储的内容字节。
    data: Bytes,
    /// 内容 MIME 类型。
    mime_type: Option<String>,
    /// 上传时的原始文件名。
    original_filename: Option<String>,
    /// 内容校验和集合。
    checksums: Vec<Checksum>,
}

/// 执行资源动作的用例命令。
#[derive(Debug, Clone)]
pub struct ExecuteResourceAction {
    action: crate::port::ResourceAction,
    input: serde_json::Value,
}

impl ExecuteResourceAction {
    /// 创建资源动作执行命令。
    pub fn new(action: impl Into<crate::port::ResourceAction>) -> Self {
        Self {
            action: action.into(),
            input: serde_json::Value::Object(Default::default()),
        }
    }

    /// 设置动作输入。
    pub fn with_input(mut self, input: serde_json::Value) -> Self {
        self.input = input;
        self
    }
}

impl UploadResourceContent {
    /// 创建命令，默认使用 `core:file`、活跃状态和空元数据。
    ///
    /// 该命令当前以 `Bytes` 承载完整内容，适合普通文件和测试场景。后续如需支持超大文件，
    /// 可以在保持 usecase 语义不变的前提下扩展流式上传端口。
    pub fn new(name: impl Into<String>, storage_key: StorageKey, data: Bytes) -> Self {
        Self {
            name: name.into(),
            kind: None,
            status: ResourceStatus::default(),
            metadata: ResourceMetadata::default(),
            storage_key,
            data,
            mime_type: None,
            original_filename: None,
            checksums: Vec::new(),
        }
    }

    /// 设置资源类型。
    ///
    /// 未调用该方法时，会按内容 MIME 类型、文件名和插件 action 规则自动推断资源类型。
    pub fn with_kind(mut self, kind: impl Into<ResourceKind>) -> Self {
        self.kind = Some(kind.into());
        self
    }

    /// 设置初始生命周期状态。
    ///
    /// 未调用该方法时，资源状态默认为 `ResourceStatus::Active`。
    pub fn with_status(mut self, status: ResourceStatus) -> Self {
        self.status = status;
        self
    }

    /// 设置初始资源元数据。
    ///
    /// 未调用该方法时，资源元数据默认为服务端定义的空元数据结构。
    pub fn with_metadata(mut self, metadata: impl Into<ResourceMetadata>) -> Self {
        self.metadata = metadata.into();
        self
    }

    /// 设置内容 MIME 类型。
    ///
    /// 该值会在构建 `ResourceContent` 时去除首尾空白并校验。
    pub fn with_mime_type(mut self, mime_type: impl Into<String>) -> Self {
        self.mime_type = Some(mime_type.into());
        self
    }

    /// 设置上传时的原始文件名。
    ///
    /// 该值仅作为内容描述信息保存，不参与对象存储路径生成。
    pub fn with_original_filename(mut self, original_filename: impl Into<String>) -> Self {
        self.original_filename = Some(original_filename.into());
        self
    }

    /// 追加一个内容校验和。
    ///
    /// 校验和应在传入前通过 `Checksum` 的构造函数完成格式校验。
    pub fn with_checksum(mut self, checksum: Checksum) -> Self {
        self.checksums.push(checksum);
        self
    }

    /// 批量追加内容校验和。
    ///
    /// 该方法不会去重；如果调用方传入重复校验和，会按原样保存到资源内容引用中。
    pub fn with_checksums(mut self, checksums: impl IntoIterator<Item = Checksum>) -> Self {
        self.checksums.extend(checksums);
        self
    }
}

/// 流式上传内容并创建资源的用例命令。
///
/// 该命令用于大文件上传。内容以 `BlobByteStream` 传入，service 会逐块写入对象存储，
/// 避免把完整文件一次性加载到内存中。
pub struct UploadResourceContentStream {
    /// 资源展示名。
    name: String,
    /// 资源类型；未设置时会按内容特征自动推断，推断失败时使用 `core:file`。
    kind: Option<ResourceKind>,
    /// 初始生命周期状态。
    status: ResourceStatus,
    /// 初始资源元数据。
    metadata: ResourceMetadata,
    /// 内容在对象存储中的定位键。
    storage_key: StorageKey,
    /// 需要写入对象存储的内容字节流。
    data: BlobByteStream,
    /// 内容 MIME 类型。
    mime_type: Option<String>,
    /// 上传时的原始文件名。
    original_filename: Option<String>,
    /// 内容校验和集合。
    checksums: Vec<Checksum>,
}

impl UploadResourceContentStream {
    /// 创建流式上传命令，默认自动推断资源类型、使用活跃状态和空元数据。
    pub fn new(name: impl Into<String>, storage_key: StorageKey, data: BlobByteStream) -> Self {
        Self {
            name: name.into(),
            kind: None,
            status: ResourceStatus::default(),
            metadata: ResourceMetadata::default(),
            storage_key,
            data,
            mime_type: None,
            original_filename: None,
            checksums: Vec::new(),
        }
    }

    /// 设置资源类型。
    pub fn with_kind(mut self, kind: impl Into<ResourceKind>) -> Self {
        self.kind = Some(kind.into());
        self
    }

    /// 设置初始生命周期状态。
    pub fn with_status(mut self, status: ResourceStatus) -> Self {
        self.status = status;
        self
    }

    /// 设置初始资源元数据。
    pub fn with_metadata(mut self, metadata: impl Into<ResourceMetadata>) -> Self {
        self.metadata = metadata.into();
        self
    }

    /// 设置内容 MIME 类型。
    pub fn with_mime_type(mut self, mime_type: impl Into<String>) -> Self {
        self.mime_type = Some(mime_type.into());
        self
    }

    /// 设置上传时的原始文件名。
    pub fn with_original_filename(mut self, original_filename: impl Into<String>) -> Self {
        self.original_filename = Some(original_filename.into());
        self
    }

    /// 追加一个内容校验和。
    pub fn with_checksum(mut self, checksum: Checksum) -> Self {
        self.checksums.push(checksum);
        self
    }

    /// 批量追加内容校验和。
    pub fn with_checksums(mut self, checksums: impl IntoIterator<Item = Checksum>) -> Self {
        self.checksums.extend(checksums);
        self
    }
}

/// 更新资源的用例命令。
#[derive(Debug, Clone, Default)]
pub struct UpdateResource {
    /// 新资源名称。
    name: Option<String>,
    /// 新资源类型。
    kind: Option<ResourceKind>,
    /// 新生命周期状态。
    status: Option<ResourceStatus>,
    /// 新资源元数据。
    metadata: Option<ResourceMetadata>,
    /// 是否从软删除状态恢复。
    restore: bool,
}

impl UpdateResource {
    /// 创建空更新命令。
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置资源名称。
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// 设置资源类型。
    pub fn with_kind(mut self, kind: impl Into<ResourceKind>) -> Self {
        self.kind = Some(kind.into());
        self
    }

    /// 设置资源状态。
    pub fn with_status(mut self, status: ResourceStatus) -> Self {
        self.status = Some(status);
        self
    }

    /// 设置资源元数据。
    pub fn with_metadata(mut self, metadata: impl Into<ResourceMetadata>) -> Self {
        self.metadata = Some(metadata.into());
        self
    }

    /// 设置是否恢复软删除资源。
    pub fn with_restore(mut self, restore: bool) -> Self {
        self.restore = restore;
        self
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
    fn new(id: ResourceId, name: String, kind: ResourceKind, view: PluginView) -> Self {
        Self {
            id,
            name,
            kind,
            view,
        }
    }

    /// 返回资源 ID。
    pub fn id(&self) -> ResourceId {
        self.id
    }

    /// 返回资源名称。
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 返回资源类型。
    pub fn kind(&self) -> &ResourceKind {
        &self.kind
    }

    /// 返回插件 View。
    pub fn view(&self) -> &PluginView {
        &self.view
    }
}

/// 资源当前可执行动作。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResourceActions {
    download_content: bool,
    read: bool,
    view_inline: bool,
    preview: bool,
    thumbnail: bool,
    available_actions: Vec<crate::port::ResourceActionDefinition>,
}

impl ResourceActions {
    fn new(
        download_content: bool,
        read: bool,
        view_inline: bool,
        preview: bool,
        thumbnail: bool,
        available_actions: Vec<crate::port::ResourceActionDefinition>,
    ) -> Self {
        Self {
            download_content,
            read,
            view_inline,
            preview,
            thumbnail,
            available_actions,
        }
    }

    /// 是否允许下载原始内容。
    pub fn download_content(&self) -> bool {
        self.download_content
    }

    /// 是否允许在线阅读文本。
    pub fn read(&self) -> bool {
        self.read
    }

    /// 是否允许以内联方式查看原始内容。
    pub fn view_inline(&self) -> bool {
        self.view_inline
    }

    /// 是否允许预览。
    pub fn preview(&self) -> bool {
        self.preview
    }

    /// 是否允许生成或读取缩略图。
    pub fn thumbnail(&self) -> bool {
        self.thumbnail
    }

    /// 返回当前资源可执行的插件动作。
    pub fn available_actions(&self) -> &[crate::port::ResourceActionDefinition] {
        &self.available_actions
    }
}

/// 预览资源结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourcePreview {
    content_type: String,
    content: Bytes,
}

impl ResourcePreview {
    fn new(content_type: String, content: Bytes) -> Self {
        Self {
            content_type,
            content,
        }
    }

    /// 返回内容类型。
    pub fn content_type(&self) -> &str {
        &self.content_type
    }

    /// 返回预览内容。
    pub fn content(&self) -> &Bytes {
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
    fn new(content_type: String, content_length: Option<u64>, content: BlobByteStream) -> Self {
        Self {
            content_type,
            content_length,
            content,
        }
    }

    /// 返回内容类型。
    pub fn content_type(&self) -> &str {
        &self.content_type
    }

    /// 返回内容长度。
    pub fn content_length(&self) -> Option<u64> {
        self.content_length
    }

    /// 消费并返回预览内容流。
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
    fn new(content_type: String, content: Bytes) -> Self {
        Self {
            content_type,
            content,
        }
    }

    /// 返回内容类型。
    pub fn content_type(&self) -> &str {
        &self.content_type
    }

    /// 返回缩略图内容。
    pub fn content(&self) -> &Bytes {
        &self.content
    }
}

/// 资源应用服务。
///
/// 该服务是外部调用资源核心能力的主要入口。它负责协调 `Resource` 聚合、
/// `ResourceRepository` 和 `BlobStorage`，但不拥有具体数据库或对象存储实现。
///
/// 对象存储和数据库之间没有分布式事务。本服务会在关键流程中做必要的顺序控制和
/// 最小补偿，但调用方仍应根据业务需要在更外层增加重试、任务补偿或审计机制。
#[derive(Clone)]
pub struct ResourceService {
    /// 资源聚合仓储端口。
    repository: Arc<dyn ResourceRepository>,
    /// 对象存储端口。
    blob_storage: Arc<dyn BlobStorage>,
    /// 资源类型注册表。
    kind_registry: Arc<dyn ResourceKindRegistry>,
    /// 全局资源动作注册表。
    action_registry: Option<Arc<dyn ResourceActionRegistry>>,
    /// 插件资源动作执行器。
    action_executor: Option<Arc<dyn ResourceActionExecutor>>,
}

impl ResourceService {
    /// 创建资源应用服务。
    ///
    /// `repository` 和 `blob_storage` 通常由应用启动层根据配置创建，例如 SQLite + Fs、
    /// Postgres + S3 等组合。这里使用 trait object 是为了让应用层可以替换具体实现。
    pub fn new(
        repository: Arc<dyn ResourceRepository>,
        blob_storage: Arc<dyn BlobStorage>,
        kind_registry: Arc<dyn ResourceKindRegistry>,
    ) -> Self {
        Self {
            repository,
            blob_storage,
            kind_registry,
            action_registry: None,
            action_executor: None,
        }
    }

    /// 创建带资源动作执行器的资源应用服务。
    pub fn new_with_action_executor(
        repository: Arc<dyn ResourceRepository>,
        blob_storage: Arc<dyn BlobStorage>,
        kind_registry: Arc<dyn ResourceKindRegistry>,
        action_executor: Arc<dyn ResourceActionExecutor>,
    ) -> Self {
        Self {
            repository,
            blob_storage,
            kind_registry,
            action_registry: None,
            action_executor: Some(action_executor),
        }
    }

    /// 创建带全局资源动作注册表和动作执行器的资源应用服务。
    pub fn new_with_action_registry_and_executor(
        repository: Arc<dyn ResourceRepository>,
        blob_storage: Arc<dyn BlobStorage>,
        kind_registry: Arc<dyn ResourceKindRegistry>,
        action_registry: Arc<dyn ResourceActionRegistry>,
        action_executor: Arc<dyn ResourceActionExecutor>,
    ) -> Self {
        Self {
            repository,
            blob_storage,
            kind_registry,
            action_registry: Some(action_registry),
            action_executor: Some(action_executor),
        }
    }

    /// 创建纯元数据资源。
    ///
    /// 该 usecase 只保存资源聚合，不写入对象存储。成功时返回已经保存的 `Resource`，
    /// 其中包含新生成的 `ResourceId`、创建时间和更新时间。
    ///
    /// 可能返回的错误包括领域校验错误和仓储保存错误。
    pub async fn create_resource(&self, command: CreateResource) -> Result<Resource, CoreError> {
        let kind = self.validate_registered_kind(command.kind)?;
        let resource =
            build_resource(command.name, Some(kind), command.status, command.metadata).build()?;

        self.repository.save(&resource).await?;

        Ok(resource)
    }

    /// 上传对象内容并创建资源。
    ///
    /// 该 usecase 会先完成领域对象构建和校验，再把内容写入 `BlobStorage`，最后通过
    /// `ResourceRepository` 保存资源聚合。
    ///
    /// 如果对象写入成功但资源保存失败，本方法会尝试删除刚写入的对象内容。该补偿删除是
    /// best-effort：补偿失败不会覆盖原始仓储错误，调用方可以通过日志或外层任务继续清理。
    ///
    /// 成功时返回已经保存的 `Resource`，其中的 `content` 指向刚写入的对象。
    pub async fn upload_resource_content(
        &self,
        command: UploadResourceContent,
    ) -> Result<Resource, CoreError> {
        let UploadResourceContent {
            name,
            kind,
            status,
            metadata,
            storage_key,
            data,
            mime_type,
            original_filename,
            checksums,
        } = command;
        let kind = self.resolve_content_kind(
            kind,
            mime_type.as_deref(),
            original_filename
                .as_deref()
                .or_else(|| Some(storage_key.as_str())),
        )?;

        verify_bytes_checksums(&data, &checksums)?;

        let content = build_content(
            storage_key.clone(),
            data.len() as u64,
            mime_type,
            original_filename,
            checksums,
        )?;
        let resource = build_resource(name, Some(kind), status, metadata)
            .with_content(content)
            .build()?;

        self.blob_storage.put_if_absent(&storage_key, data).await?;

        if let Err(error) = self.repository.save(&resource).await {
            let _ = self.blob_storage.delete(&storage_key).await;
            return Err(error);
        }

        Ok(resource)
    }

    /// 流式上传对象内容并创建资源。
    ///
    /// 该 usecase 面向大文件上传。内容会以 chunk 流的形式写入 `BlobStorage`，不会在
    /// service 层聚合成完整 `Bytes`。写入完成后，service 使用存储端口返回的实际字节数
    /// 构建 `ResourceContent` 并保存资源聚合。
    ///
    /// 如果资源保存失败，本方法会尝试删除刚写入的对象内容。该补偿删除是 best-effort，
    /// 不会覆盖原始仓储错误。
    pub async fn upload_resource_content_stream(
        &self,
        command: UploadResourceContentStream,
    ) -> Result<Resource, CoreError> {
        let UploadResourceContentStream {
            name,
            kind,
            status,
            metadata,
            storage_key,
            data,
            mime_type,
            original_filename,
            checksums,
        } = command;
        let kind = self.resolve_content_kind(
            kind,
            mime_type.as_deref(),
            original_filename
                .as_deref()
                .or_else(|| Some(storage_key.as_str())),
        )?;

        let resource_builder = build_resource(name, Some(kind), status, metadata);
        resource_builder.clone().build()?;
        build_content(
            storage_key.clone(),
            0,
            mime_type.clone(),
            original_filename.clone(),
            checksums.clone(),
        )?;

        let (data, sha256_state) = stream_with_checksum_tracking(data, &checksums);
        let write_result = self
            .blob_storage
            .put_stream_if_absent(&storage_key, data)
            .await?;
        if let Err(error) = verify_tracked_checksums(sha256_state, &checksums) {
            let _ = self.blob_storage.delete(&storage_key).await;
            return Err(error);
        }
        let content = build_content(
            storage_key.clone(),
            write_result.bytes_written(),
            mime_type,
            original_filename,
            checksums,
        )?;
        let resource = resource_builder.with_content(content).build()?;

        if let Err(error) = self.repository.save(&resource).await {
            let _ = self.blob_storage.delete(&storage_key).await;
            return Err(error);
        }

        Ok(resource)
    }

    /// 按 ID 查找资源。
    ///
    /// 找不到资源或资源已经软删除时返回 `Ok(None)`。维护类操作需要读取软删除资源时，
    /// 应使用专门的恢复或物理删除用例。
    pub async fn find_resource(&self, id: &ResourceId) -> Result<Option<Resource>, CoreError> {
        Ok(self
            .repository
            .find_by_id(id)
            .await?
            .filter(|resource| !resource.is_deleted()))
    }

    /// 分页列出资源。
    pub async fn list_resources(&self, query: ListResources) -> Result<ResourcePage, CoreError> {
        if let Some(kind) = query.kind() {
            self.ensure_kind_registered(kind)?;
        }

        self.repository.list(&query).await
    }

    /// 计算资源当前可执行动作。
    ///
    /// 该方法统一封装资源内容状态和注册 kind 能力，供不同应用入口复用，
    /// 避免在 HTTP、CLI、TUI 中重复拼装判断逻辑。
    pub fn describe_resource_actions(
        &self,
        resource: &Resource,
    ) -> Result<ResourceActions, CoreError> {
        let definition = self.require_kind_definition(resource.kind())?;
        let Some(content) = resource.content() else {
            return Ok(ResourceActions::default());
        };
        if resource.is_deleted() {
            return Ok(ResourceActions::default());
        }

        let mime_type = content.mime_type();
        let storage_key = Some(content.key().as_str());
        let declared_actions = self.actions_for_resource(resource, &definition);
        let has_declared_action = |action: &str| {
            declared_actions.iter().any(|definition| {
                definition.id().as_str() == action
                    && definition.matches_content(mime_type, storage_key)
            })
        };
        let supports_preview = has_declared_action(crate::port::ResourceAction::PREVIEW);
        let read = has_declared_action(crate::port::ResourceAction::READ);
        let view_inline = has_declared_action(crate::port::ResourceAction::VIEW_INLINE);
        let thumbnail = has_declared_action(crate::port::ResourceAction::THUMBNAIL);
        let mut available_actions = Vec::new();
        for action in &declared_actions {
            let enabled = match action.id().as_str() {
                crate::port::ResourceAction::DOWNLOAD_CONTENT => true,
                crate::port::ResourceAction::READ => read,
                crate::port::ResourceAction::VIEW_INLINE => view_inline,
                crate::port::ResourceAction::PREVIEW => supports_preview,
                crate::port::ResourceAction::THUMBNAIL => thumbnail,
                _ => action.matches_resource(resource.kind().as_str(), mime_type, storage_key),
            };

            if enabled {
                available_actions.push(action.clone());
            }
        }

        if !available_actions
            .iter()
            .any(|action| action.id().as_str() == crate::port::ResourceAction::DOWNLOAD_CONTENT)
        {
            available_actions.insert(
                0,
                crate::port::ResourceActionDefinition::new(
                    crate::port::ResourceAction::DOWNLOAD_CONTENT,
                    "Download",
                ),
            );
        }

        Ok(ResourceActions::new(
            true,
            read,
            view_inline,
            supports_preview,
            thumbnail,
            available_actions,
        ))
    }

    /// 更新资源基础信息、元数据、状态，或恢复软删除资源。
    pub async fn update_resource(
        &self,
        id: &ResourceId,
        command: UpdateResource,
    ) -> Result<Option<Resource>, CoreError> {
        let Some(mut resource) = self.repository.find_by_id(id).await? else {
            return Ok(None);
        };

        if command.restore {
            resource.restore();
        }

        if let Some(name) = command.name {
            resource.rename(name)?;
        }

        if let Some(kind) = command.kind {
            resource.change_kind(self.validate_registered_kind(Some(kind))?)?;
        }

        if let Some(status) = command.status {
            match status {
                ResourceStatus::Active => resource.activate()?,
                ResourceStatus::Archived => resource.archive()?,
            }
        }

        if let Some(metadata) = command.metadata {
            resource.set_metadata(metadata)?;
        }

        self.repository.save(&resource).await?;

        Ok(Some(resource))
    }

    /// 读取资源对应的对象内容。
    ///
    /// 该 usecase 会先读取资源聚合，再根据资源内容引用读取对象存储。
    ///
    /// 以下情况统一返回 `Ok(None)`：
    /// - 资源不存在。
    /// - 资源已软删除。
    /// - 资源没有内容引用。
    /// - 内容引用存在，但对象存储中没有对应对象。
    ///
    /// 对象存储自身故障会返回 `Err(CoreError::Storage { .. })`。
    pub async fn get_resource_content(&self, id: &ResourceId) -> Result<Option<Bytes>, CoreError> {
        let Some(resource) = self.repository.find_by_id(id).await? else {
            return Ok(None);
        };

        if resource.is_deleted() {
            return Ok(None);
        }

        let Some(content) = resource.content() else {
            return Ok(None);
        };

        self.blob_storage.get(content.key()).await
    }

    /// 读取资源的可阅读 View。
    ///
    /// 该 usecase 统一负责 `read` action 校验、对象内容读取和插件调度，供 HTTP、CLI、
    /// TUI 等应用入口复用。具体格式解析由插件负责。
    ///
    /// 找不到资源、资源已删除或没有内容时返回 `Ok(None)`。资源类型不支持阅读，或内容格式
    /// 没有插件 handler 时返回 `Err(CoreError::Configuration { .. })`。
    pub async fn read_resource(
        &self,
        id: &ResourceId,
    ) -> Result<Option<ReadableResource>, CoreError> {
        let Some(output) = self
            .execute_declared_resource_action(
                id,
                crate::port::ResourceAction::READ.into(),
                serde_json::Value::Null,
            )
            .await?
        else {
            return Ok(None);
        };
        let Some(resource) = self.find_resource(id).await? else {
            return Ok(None);
        };

        Ok(Some(ReadableResource::new(
            resource.id(),
            resource.name().to_string(),
            resource.kind().clone(),
            output.output().view.clone(),
        )))
    }

    /// 执行资源类型声明的插件动作。
    ///
    /// 核心负责资源存在性、删除状态、kind/action 声明、访问边界和对象内容加载；具体 wasm
    /// 运行时由 `ResourceActionExecutor` 端口承接。
    pub async fn execute_resource_action(
        &self,
        id: &ResourceId,
        command: ExecuteResourceAction,
    ) -> Result<Option<ResourceActionOutput>, CoreError> {
        self.execute_declared_resource_action(id, command.action, command.input)
            .await
    }

    async fn execute_declared_resource_action(
        &self,
        id: &ResourceId,
        action_id: crate::port::ResourceAction,
        input: serde_json::Value,
    ) -> Result<Option<ResourceActionOutput>, CoreError> {
        let Some(mut resource) = self.find_resource(id).await? else {
            return Ok(None);
        };
        let definition = self.require_kind_definition(resource.kind())?;
        let declared_actions = self.actions_for_resource(&resource, &definition);
        let Some(action) = declared_actions
            .iter()
            .find(|action| {
                let content_ref = resource.content();
                action.id().as_str() == action_id.as_str()
                    && action.matches_resource(
                        resource.kind().as_str(),
                        content_ref.and_then(|content| content.mime_type()),
                        content_ref.map(|content| content.key().as_str()),
                    )
            })
            .cloned()
        else {
            return Err(CoreError::configuration(format!(
                "resource kind `{}` does not support action `{}`",
                resource.kind(),
                action_id
            )));
        };
        let content = match resource.content() {
            Some(content_ref)
                if action.handler().is_none()
                    || should_load_action_content(&action, content_ref.size()) =>
            {
                self.blob_storage.get(content_ref.key()).await?
            }
            _ => None,
        };
        let Some(handler) = action.handler() else {
            return crate::action::builtin::execute(resource, action_id, content).map(Some);
        };
        let Some(executor) = &self.action_executor else {
            return Err(CoreError::configuration(
                "resource action executor is not configured",
            ));
        };
        let access = action.access();
        let request = ResourceActionRequest::new(
            resource.clone(),
            action_id,
            handler,
            access,
            action.content_delivery(),
            input,
            content,
        );

        let output = executor.execute(request).await?;
        self.apply_action_effects(&mut resource, &output, access)
            .await?;

        Ok(Some(output))
    }

    async fn apply_action_effects(
        &self,
        resource: &mut Resource,
        output: &ResourceActionOutput,
        access: crate::port::ResourceActionAccess,
    ) -> Result<(), CoreError> {
        if output.output().effects.is_empty() {
            return Ok(());
        }
        if !matches!(access, crate::port::ResourceActionAccess::ReadWrite) {
            return Err(CoreError::configuration(format!(
                "action `{}` returned effects without write access",
                output.action()
            )));
        }

        for effect in &output.output().effects {
            match effect {
                PluginActionEffect::ReplaceContent(effect) => {
                    let Some(current_content) = resource.content().cloned() else {
                        return Err(CoreError::configuration(format!(
                            "action `{}` cannot replace missing resource content",
                            output.action()
                        )));
                    };
                    if !matches!(effect.encoding, PluginContentEncoding::Base64) {
                        return Err(CoreError::configuration(format!(
                            "action `{}` returned unsupported replace_content encoding",
                            output.action()
                        )));
                    }
                    let data = BASE64_STANDARD
                        .decode(effect.data.as_bytes())
                        .map(Bytes::from)
                        .map_err(|error| {
                            CoreError::configuration(format!(
                                "action `{}` returned invalid replace_content base64: {error}",
                                output.action()
                            ))
                        })?;
                    let checksums = plugin_checksums(&effect.checksum, &data)?;
                    let content = build_content(
                        current_content.key().clone(),
                        data.len() as u64,
                        effect
                            .mime_type
                            .clone()
                            .or_else(|| current_content.mime_type().map(str::to_string)),
                        effect
                            .original_filename
                            .clone()
                            .or_else(|| current_content.original_filename().map(str::to_string)),
                        checksums,
                    )?;

                    self.blob_storage.put(current_content.key(), data).await?;
                    resource.attach_content(content)?;
                    self.repository.save(resource).await?;
                }
            }
        }

        Ok(())
    }

    /// 读取资源预览内容。
    pub async fn preview_resource(
        &self,
        id: &ResourceId,
    ) -> Result<Option<ResourcePreview>, CoreError> {
        let Some(output) = self
            .execute_declared_resource_action(
                id,
                crate::port::ResourceAction::PREVIEW.into(),
                serde_json::Value::Null,
            )
            .await?
        else {
            return Ok(None);
        };
        let (content_type, content) = crate::action::builtin::decode_media_view(
            crate::port::ResourceAction::PREVIEW,
            &output.output().view,
        )?;

        Ok(Some(ResourcePreview::new(content_type, content)))
    }

    /// 返回资源预览内容流。
    pub async fn preview_resource_stream(
        &self,
        id: &ResourceId,
    ) -> Result<Option<ResourcePreviewStream>, CoreError> {
        let Some(resource) = self.find_resource(id).await? else {
            return Ok(None);
        };
        let definition = self.require_kind_definition(resource.kind())?;
        let declared_actions = self.actions_for_resource(&resource, &definition);
        let content_ref = resource.content();
        let Some(action) = declared_actions.iter().find(|action| {
            action.id().as_str() == crate::port::ResourceAction::PREVIEW
                && action.matches_resource(
                    resource.kind().as_str(),
                    content_ref.and_then(|content| content.mime_type()),
                    content_ref.map(|content| content.key().as_str()),
                )
        }) else {
            return Err(CoreError::configuration(format!(
                "resource kind `{}` does not support action `preview`",
                resource.kind()
            )));
        };
        if action.handler().is_some() {
            return Err(CoreError::configuration(
                "plugin preview actions must be executed through the action endpoint",
            ));
        }
        let Some(content_ref) = resource.content() else {
            return Err(CoreError::not_found(
                "resource content",
                resource.id().to_string(),
            ));
        };
        let Some(content) = self.blob_storage.get_stream(content_ref.key()).await? else {
            return Err(CoreError::not_found(
                "resource content",
                resource.id().to_string(),
            ));
        };

        Ok(Some(ResourcePreviewStream::new(
            crate::action::builtin::content_type_for_media(content_ref),
            Some(content_ref.size()),
            content,
        )))
    }

    /// 读取资源缩略图内容。
    pub async fn thumbnail_resource(
        &self,
        id: &ResourceId,
    ) -> Result<Option<ResourceThumbnail>, CoreError> {
        let Some(output) = self
            .execute_declared_resource_action(
                id,
                crate::port::ResourceAction::THUMBNAIL.into(),
                serde_json::Value::Null,
            )
            .await?
        else {
            return Ok(None);
        };
        let (content_type, content) = crate::action::builtin::decode_media_view(
            crate::port::ResourceAction::THUMBNAIL,
            &output.output().view,
        )?;

        Ok(Some(ResourceThumbnail::new(content_type, content)))
    }

    /// 软删除资源。
    ///
    /// 软删除只更新资源聚合状态并保存到仓储，不删除对象存储中的内容。这样可以保留恢复、
    /// 审计或异步清理的空间。
    ///
    /// 找不到资源时返回 `Ok(None)`；找到资源时返回保存后的资源状态。重复软删除同一资源是
    /// 幂等的，领域模型不会反复刷新删除时间。
    pub async fn soft_delete_resource(
        &self,
        id: &ResourceId,
    ) -> Result<Option<Resource>, CoreError> {
        let Some(mut resource) = self.repository.find_by_id(id).await? else {
            return Ok(None);
        };

        resource.soft_delete();
        self.repository.save(&resource).await?;

        Ok(Some(resource))
    }

    /// 物理移除资源及其对象内容。
    ///
    /// 该 usecase 用于维护任务或明确需要硬删除的场景，不是默认业务删除入口。
    ///
    /// 执行顺序是先删除对象内容，再物理移除资源记录。这样即使仓储移除失败，调用方也可以
    /// 安全重试，因为 `BlobStorage::delete` 被定义为幂等操作。
    ///
    /// 返回值表示是否找到并尝试移除了资源：资源不存在时返回 `Ok(false)`，找到并完成移除时
    /// 返回 `Ok(true)`。
    pub async fn remove_resource(&self, id: &ResourceId) -> Result<bool, CoreError> {
        let Some(resource) = self.repository.find_by_id(id).await? else {
            return Ok(false);
        };

        if let Some(content) = resource.content() {
            self.blob_storage.delete(content.key()).await?;
        }

        self.repository.remove(id).await?;

        Ok(true)
    }

    fn validate_registered_kind(
        &self,
        kind: Option<ResourceKind>,
    ) -> Result<ResourceKind, CoreError> {
        let kind = kind.unwrap_or_else(fallback_resource_kind);
        self.ensure_kind_registered(&kind)?;
        Ok(kind)
    }

    fn validate_content_kind(&self, kind: Option<ResourceKind>) -> Result<ResourceKind, CoreError> {
        let kind = self.validate_registered_kind(kind)?;
        let definition = self.require_kind_definition(&kind)?;
        if !definition.supports_content() {
            return Err(CoreError::configuration(format!(
                "resource kind `{kind}` does not support content upload"
            )));
        }

        Ok(kind)
    }

    fn resolve_content_kind(
        &self,
        kind: Option<ResourceKind>,
        mime_type: Option<&str>,
        storage_key: Option<&str>,
    ) -> Result<ResourceKind, CoreError> {
        let kind = kind.or_else(|| {
            self.kind_registry
                .detect_content_kind(mime_type, storage_key)
        });
        self.validate_content_kind(kind)
    }

    fn ensure_kind_registered(&self, kind: &ResourceKind) -> Result<(), CoreError> {
        if self.kind_registry.supports(kind) {
            return Ok(());
        }

        Err(CoreError::configuration(format!(
            "unsupported resource kind `{kind}`"
        )))
    }

    fn require_kind_definition(
        &self,
        kind: &ResourceKind,
    ) -> Result<crate::port::ResourceKindDefinition, CoreError> {
        self.kind_registry
            .get(kind)
            .ok_or_else(|| CoreError::configuration(format!("unsupported resource kind `{kind}`")))
    }

    fn actions_for_resource(
        &self,
        resource: &Resource,
        definition: &crate::port::ResourceKindDefinition,
    ) -> Vec<crate::port::ResourceActionDefinition> {
        self.action_registry
            .as_ref()
            .map(|registry| registry.actions_for_resource(resource))
            .unwrap_or_else(|| definition.actions().to_vec())
    }
}

fn should_load_action_content(action: &crate::port::ResourceActionDefinition, size: u64) -> bool {
    match action.content_delivery() {
        crate::port::ResourceActionContentDelivery::Inline => true,
        crate::port::ResourceActionContentDelivery::Url => action.requires_content(),
        crate::port::ResourceActionContentDelivery::Auto => size <= MAX_INLINE_PLUGIN_CONTENT_BYTES,
    }
}

fn build_resource(
    name: String,
    kind: Option<ResourceKind>,
    status: ResourceStatus,
    metadata: ResourceMetadata,
) -> crate::domain::ResourceBuilder {
    let mut builder = Resource::builder(name)
        .with_status(status)
        .with_metadata(metadata);

    if let Some(kind) = kind {
        builder = builder.with_kind(kind);
    }

    builder
}

fn fallback_resource_kind() -> ResourceKind {
    ResourceKind::from("core:file")
}

fn build_content(
    storage_key: StorageKey,
    size: u64,
    mime_type: Option<String>,
    original_filename: Option<String>,
    checksums: Vec<Checksum>,
) -> Result<ResourceContent, CoreError> {
    let mut content = ResourceContent::builder(storage_key, size);

    if let Some(mime_type) = mime_type {
        content = content.with_mime_type(mime_type);
    }

    if let Some(original_filename) = original_filename {
        content = content.with_original_filename(original_filename);
    }

    Ok(content.with_checksums(checksums).build()?)
}

fn verify_bytes_checksums(data: &Bytes, checksums: &[Checksum]) -> Result<(), CoreError> {
    if let Some(expected) = sha256_checksum(checksums) {
        let actual = hex_sha256(data);

        if !actual.eq_ignore_ascii_case(expected.value()) {
            return Err(CoreError::conflict("sha256 checksum mismatch"));
        }
    }

    Ok(())
}

fn plugin_checksums(
    checksums: &[asset_plugin_api::PluginChecksum],
    data: &Bytes,
) -> Result<Vec<Checksum>, CoreError> {
    if checksums.is_empty() {
        return Ok(vec![Checksum::sha256(hex_sha256(data))?]);
    }

    let mut converted = Vec::with_capacity(checksums.len());
    for checksum in checksums {
        match checksum.kind.as_str() {
            "sha256" => converted.push(Checksum::sha256(checksum.value.clone())?),
            other => {
                return Err(CoreError::configuration(format!(
                    "unsupported plugin checksum kind `{other}`"
                )));
            }
        }
    }
    verify_bytes_checksums(data, &converted)?;
    Ok(converted)
}

fn stream_with_checksum_tracking(
    data: BlobByteStream,
    checksums: &[Checksum],
) -> (BlobByteStream, Option<Arc<std::sync::Mutex<Sha256>>>) {
    if sha256_checksum(checksums).is_none() {
        return (data, None);
    }

    let state = Arc::new(std::sync::Mutex::new(Sha256::new()));
    let stream_state = state.clone();
    let stream = data.map(move |chunk| {
        if let Ok(chunk) = &chunk {
            stream_state
                .lock()
                .expect("sha256 mutex should not be poisoned")
                .update(chunk);
        }

        chunk
    });

    (Box::pin(stream), Some(state))
}

fn verify_tracked_checksums(
    sha256_state: Option<Arc<std::sync::Mutex<Sha256>>>,
    checksums: &[Checksum],
) -> Result<(), CoreError> {
    let Some(expected) = sha256_checksum(checksums) else {
        return Ok(());
    };
    let Some(state) = sha256_state else {
        return Ok(());
    };
    let digest = state
        .lock()
        .expect("sha256 mutex should not be poisoned")
        .clone()
        .finalize();
    let actual = hex_digest(&digest);

    if !actual.eq_ignore_ascii_case(expected.value()) {
        return Err(CoreError::conflict("sha256 checksum mismatch"));
    }

    Ok(())
}

fn sha256_checksum(checksums: &[Checksum]) -> Option<&Checksum> {
    checksums
        .iter()
        .find(|checksum| checksum.kind() == ChecksumKind::Sha256)
}

fn hex_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let digest = hasher.finalize();
    hex_digest(&digest)
}

fn hex_digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);

    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::KindMetadata;
    use crate::port::{
        BlobWriteResult, ResourceAction, ResourceActionDefinition, ResourceKindDefinition,
        ResourceKindRegistry,
    };
    use asset_plugin_api::{
        MediaView, PluginActionEffect, PluginActionOutput, PluginContentEncoding, PluginView,
        ReplaceContentEffect, TextView,
    };
    use async_trait::async_trait;
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use futures_util::StreamExt;
    use serde_json::json;
    use std::collections::HashMap;
    use std::fmt;
    use std::future::Future;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Wake, Waker};

    #[derive(Default)]
    struct InMemoryResourceRepository {
        resources: Mutex<HashMap<ResourceId, Resource>>,
        fail_next_save: Mutex<bool>,
    }

    impl InMemoryResourceRepository {
        fn fail_next_save(&self) {
            *self.fail_next_save.lock().unwrap() = true;
        }

        fn find_sync(&self, id: &ResourceId) -> Option<Resource> {
            self.resources.lock().unwrap().get(id).cloned()
        }

        fn is_empty(&self) -> bool {
            self.resources.lock().unwrap().is_empty()
        }
    }

    #[async_trait::async_trait]
    impl ResourceRepository for InMemoryResourceRepository {
        async fn save(&self, resource: &Resource) -> Result<(), CoreError> {
            if std::mem::take(&mut *self.fail_next_save.lock().unwrap()) {
                return Err(CoreError::repository("save", TestError("save failed")));
            }

            self.resources
                .lock()
                .unwrap()
                .insert(resource.id(), resource.clone());

            Ok(())
        }

        async fn find_by_id(&self, id: &ResourceId) -> Result<Option<Resource>, CoreError> {
            Ok(self.find_sync(id))
        }

        async fn list(&self, query: &ListResources) -> Result<ResourcePage, CoreError> {
            let mut resources = self
                .resources
                .lock()
                .unwrap()
                .values()
                .filter(|resource| query.include_deleted() || !resource.is_deleted())
                .filter(|resource| {
                    query
                        .kind()
                        .is_none_or(|kind| resource.kind().as_str() == kind.as_str())
                })
                .filter(|resource| {
                    query.tag().is_none_or(|tag| {
                        resource.metadata().tags().iter().any(|value| value == tag)
                    })
                })
                .filter(|resource| query.q().is_none_or(|q| resource.name().contains(q)))
                .cloned()
                .collect::<Vec<_>>();
            resources.sort_by_key(|resource| std::cmp::Reverse(resource.updated_at()));

            let total = resources.len() as u64;
            let items = resources
                .into_iter()
                .skip(query.offset() as usize)
                .take(query.limit() as usize)
                .collect();

            Ok(ResourcePage {
                items,
                total,
                limit: query.limit(),
                offset: query.offset(),
            })
        }

        async fn remove(&self, id: &ResourceId) -> Result<(), CoreError> {
            self.resources.lock().unwrap().remove(id);
            Ok(())
        }
    }

    #[derive(Default)]
    struct InMemoryResourceKindRegistry {
        definitions: Vec<ResourceKindDefinition>,
    }

    impl InMemoryResourceKindRegistry {
        fn with_definitions(definitions: Vec<ResourceKindDefinition>) -> Self {
            Self { definitions }
        }
    }

    impl ResourceKindRegistry for InMemoryResourceKindRegistry {
        fn list(&self) -> Vec<ResourceKindDefinition> {
            self.definitions.clone()
        }
    }

    #[derive(Default)]
    struct InMemoryBlobStorage {
        objects: Mutex<HashMap<StorageKey, Bytes>>,
    }

    impl InMemoryBlobStorage {
        fn contains(&self, key: &StorageKey) -> bool {
            self.objects.lock().unwrap().contains_key(key)
        }

        fn get_sync(&self, key: &StorageKey) -> Option<Bytes> {
            self.objects.lock().unwrap().get(key).cloned()
        }
    }

    #[async_trait::async_trait]
    impl BlobStorage for InMemoryBlobStorage {
        async fn put(&self, key: &StorageKey, data: Bytes) -> Result<(), CoreError> {
            self.objects.lock().unwrap().insert(key.clone(), data);
            Ok(())
        }

        async fn put_if_absent(&self, key: &StorageKey, data: Bytes) -> Result<(), CoreError> {
            let mut objects = self.objects.lock().unwrap();
            if objects.contains_key(key) {
                return Err(CoreError::conflict(format!(
                    "storage key `{key}` already exists"
                )));
            }

            objects.insert(key.clone(), data);
            Ok(())
        }

        async fn put_stream(
            &self,
            key: &StorageKey,
            mut data: BlobByteStream,
        ) -> Result<BlobWriteResult, CoreError> {
            let mut bytes = Vec::new();

            while let Some(chunk) = data.next().await {
                bytes.extend_from_slice(&chunk?);
            }

            let bytes_written = bytes.len() as u64;
            self.objects
                .lock()
                .unwrap()
                .insert(key.clone(), Bytes::from(bytes));

            Ok(BlobWriteResult::new(bytes_written))
        }

        async fn put_stream_if_absent(
            &self,
            key: &StorageKey,
            mut data: BlobByteStream,
        ) -> Result<BlobWriteResult, CoreError> {
            let mut bytes = Vec::new();

            while let Some(chunk) = data.next().await {
                bytes.extend_from_slice(&chunk?);
            }

            let bytes_written = bytes.len() as u64;
            let mut objects = self.objects.lock().unwrap();
            if objects.contains_key(key) {
                return Err(CoreError::conflict(format!(
                    "storage key `{key}` already exists"
                )));
            }

            objects.insert(key.clone(), Bytes::from(bytes));

            Ok(BlobWriteResult::new(bytes_written))
        }

        async fn get(&self, key: &StorageKey) -> Result<Option<Bytes>, CoreError> {
            Ok(self.get_sync(key))
        }

        async fn get_stream(&self, key: &StorageKey) -> Result<Option<BlobByteStream>, CoreError> {
            Ok(self.get_sync(key).map(|content| {
                Box::pin(futures_util::stream::once(async move { Ok(content) })) as BlobByteStream
            }))
        }

        async fn delete(&self, key: &StorageKey) -> Result<(), CoreError> {
            self.objects.lock().unwrap().remove(key);
            Ok(())
        }

        async fn exists(&self, key: &StorageKey) -> Result<bool, CoreError> {
            Ok(self.contains(key))
        }
    }

    #[derive(Debug)]
    struct TestError(&'static str);

    impl fmt::Display for TestError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.0)
        }
    }

    impl std::error::Error for TestError {}

    #[derive(Debug, Default)]
    struct StaticResourceActionExecutor;

    #[async_trait]
    impl ResourceActionExecutor for StaticResourceActionExecutor {
        async fn execute(
            &self,
            request: ResourceActionRequest,
        ) -> Result<ResourceActionOutput, CoreError> {
            let view = match request.action().as_str() {
                ResourceAction::READ => PluginView::Text(TextView {
                    text: String::from_utf8(
                        request
                            .content()
                            .map(|content| content.to_vec())
                            .unwrap_or_default(),
                    )
                    .unwrap(),
                }),
                ResourceAction::PREVIEW | ResourceAction::THUMBNAIL => {
                    let content = request.content().cloned().unwrap_or_default();
                    PluginView::Media(MediaView {
                        mime_type: request
                            .resource()
                            .content()
                            .and_then(|content| content.mime_type())
                            .unwrap_or("application/octet-stream")
                            .to_string(),
                        title: Some(request.resource().name().to_string()),
                        encoding: PluginContentEncoding::Base64,
                        data: STANDARD.encode(content),
                    })
                }
                "azvs.markdown.update" => {
                    let markdown = request
                        .input()
                        .get("markdown")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let mut output = PluginActionOutput::new(PluginView::Text(TextView {
                        text: "saved".to_string(),
                    }));
                    output
                        .effects
                        .push(PluginActionEffect::ReplaceContent(ReplaceContentEffect {
                            encoding: PluginContentEncoding::Base64,
                            data: STANDARD.encode(markdown),
                            mime_type: Some("text/markdown".to_string()),
                            original_filename: Some("note.md".to_string()),
                            checksum: Vec::new(),
                        }));

                    return Ok(ResourceActionOutput::new(
                        request.resource().id(),
                        request.action().clone(),
                        output,
                    ));
                }
                action => {
                    return Err(CoreError::configuration(format!(
                        "unexpected test action `{action}`"
                    )));
                }
            };

            Ok(ResourceActionOutput::new(
                request.resource().id(),
                request.action().clone(),
                PluginActionOutput::new(view),
            ))
        }
    }

    struct NoopWaker;

    impl Wake for NoopWaker {
        fn wake(self: Arc<Self>) {}
    }

    fn block_on<F: Future>(future: F) -> F::Output {
        let waker = Waker::from(Arc::new(NoopWaker));
        let mut context = Context::from_waker(&waker);
        let mut future = std::pin::pin!(future);

        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => output,
            Poll::Pending => panic!("test future unexpectedly returned pending"),
        }
    }

    fn service() -> (
        ResourceService,
        Arc<InMemoryResourceRepository>,
        Arc<InMemoryBlobStorage>,
    ) {
        let kind_registry = Arc::new(InMemoryResourceKindRegistry::with_definitions(vec![
            ResourceKindDefinition::new(ResourceKind::default(), "Unknown", None, true),
            ResourceKindDefinition::new(
                ResourceKind::try_new("core:file").unwrap(),
                "File",
                None,
                true,
            ),
            ResourceKindDefinition::new(
                ResourceKind::try_new("doc:markdown").unwrap(),
                "Markdown",
                None,
                false,
            )
            .with_actions(vec![
                ResourceActionDefinition::new(ResourceAction::READ, "Read")
                    .with_handler("read_markdown"),
            ]),
            ResourceKindDefinition::new(
                ResourceKind::try_new("core:image").unwrap(),
                "Image",
                None,
                true,
            )
            .with_actions(vec![
                ResourceActionDefinition::new(ResourceAction::PREVIEW, "Preview")
                    .with_handler("preview_image"),
                ResourceActionDefinition::new(ResourceAction::THUMBNAIL, "Thumbnail")
                    .with_handler("thumbnail_image"),
            ]),
            ResourceKindDefinition::new(
                ResourceKind::try_new("asset:binary").unwrap(),
                "Binary",
                None,
                true,
            ),
            ResourceKindDefinition::new(
                ResourceKind::try_new("core:document").unwrap(),
                "Document",
                None,
                true,
            )
            .with_actions(vec![
                ResourceActionDefinition::new(ResourceAction::READ, "Read")
                    .with_handler("read_document"),
                ResourceActionDefinition::new("azvs.markdown.render", "Read Markdown")
                    .with_handler("render_markdown")
                    .with_when(
                        crate::port::ResourceActionWhen::new()
                            .with_mime_types(["text/markdown", "text/x-markdown"])
                            .with_extensions([".md", ".markdown"]),
                    ),
                ResourceActionDefinition::new("azvs.markdown.update", "Edit Markdown")
                    .with_handler("update_markdown")
                    .with_access(crate::port::ResourceActionAccess::ReadWrite)
                    .with_when(
                        crate::port::ResourceActionWhen::new()
                            .with_mime_types(["text/markdown", "text/x-markdown"])
                            .with_extensions([".md", ".markdown"]),
                    ),
                ResourceActionDefinition::new(ResourceAction::PREVIEW, "Preview")
                    .with_handler("preview_document"),
            ]),
            ResourceKindDefinition::new(
                ResourceKind::try_new("core:video").unwrap(),
                "Video",
                None,
                true,
            )
            .with_actions(vec![
                ResourceActionDefinition::new(ResourceAction::PREVIEW, "Preview"),
                ResourceActionDefinition::new("azvs.mp4.play", "Play MP4")
                    .with_handler("play_mp4")
                    .with_when(
                        crate::port::ResourceActionWhen::new()
                            .with_mime_types(["video/mp4"])
                            .with_extensions([".mp4"]),
                    ),
            ]),
        ]));
        let repository = Arc::new(InMemoryResourceRepository::default());
        let blob_storage = Arc::new(InMemoryBlobStorage::default());
        let service = ResourceService::new_with_action_executor(
            repository.clone(),
            blob_storage.clone(),
            kind_registry,
            Arc::new(StaticResourceActionExecutor),
        );

        (service, repository, blob_storage)
    }

    fn service_with_registry(
        kind_registry: Arc<dyn ResourceKindRegistry>,
    ) -> (
        ResourceService,
        Arc<InMemoryResourceRepository>,
        Arc<InMemoryBlobStorage>,
    ) {
        let repository = Arc::new(InMemoryResourceRepository::default());
        let blob_storage = Arc::new(InMemoryBlobStorage::default());
        let service = ResourceService::new(repository.clone(), blob_storage.clone(), kind_registry);

        (service, repository, blob_storage)
    }

    #[test]
    fn create_resource_saves_metadata_only_resource() {
        let (service, repository, _) = service();
        let metadata = ResourceMetadata::builder()
            .with_description(" Design document ")
            .with_tags(["rust", "asset"])
            .with_kind_metadata(
                KindMetadata::new("doc:markdown@1", json!({"stage": "draft"})).unwrap(),
            )
            .build()
            .unwrap();

        let resource = block_on(
            service.create_resource(
                CreateResource::new(" Design Doc ")
                    .with_kind("doc:markdown")
                    .with_metadata(metadata.clone()),
            ),
        )
        .unwrap();

        let saved = repository.find_sync(&resource.id()).unwrap();

        assert_eq!(resource.name(), "Design Doc");
        assert!(resource.kind().is("doc:markdown"));
        assert!(resource.content().is_none());
        assert_eq!(saved.metadata().description(), Some("Design document"));
        assert_eq!(saved.metadata().tags(), &["rust", "asset"]);
        assert_eq!(
            saved.metadata().kind_metadata().unwrap().data(),
            &json!({"stage": "draft"})
        );
    }

    #[test]
    fn upload_resource_content_writes_blob_then_saves_resource() {
        let (service, repository, blob_storage) = service();
        let key = StorageKey::new("assets/image.png").unwrap();
        let data = Bytes::from_static(b"image bytes");
        let checksum = Checksum::sha256(hex_sha256(&data)).unwrap();

        let resource = block_on(
            service.upload_resource_content(
                UploadResourceContent::new("image", key.clone(), data.clone())
                    .with_kind("core:image")
                    .with_mime_type(" image/png ")
                    .with_original_filename(" image.png ")
                    .with_checksum(checksum.clone()),
            ),
        )
        .unwrap();

        let saved = repository.find_sync(&resource.id()).unwrap();
        let content = saved.content().unwrap();

        assert_eq!(content.key(), &key);
        assert_eq!(content.size(), data.len() as u64);
        assert_eq!(content.mime_type(), Some("image/png"));
        assert_eq!(content.original_filename(), Some("image.png"));
        assert_eq!(content.checksums(), &[checksum]);
        assert_eq!(blob_storage.get_sync(&key), Some(data));
    }

    #[test]
    fn upload_resource_content_detects_parent_kind_from_action_rules() {
        let (service, repository, _) = service();
        let key = StorageKey::new("docs/readme.md").unwrap();

        let resource = block_on(
            service.upload_resource_content(
                UploadResourceContent::new("readme", key, Bytes::from_static(b"# Readme"))
                    .with_mime_type("text/plain")
                    .with_original_filename("README.md"),
            ),
        )
        .unwrap();

        let saved = repository.find_sync(&resource.id()).unwrap();

        assert!(saved.kind().is("core:document"));
    }

    #[test]
    fn upload_resource_content_rejects_checksum_mismatch() {
        let (service, repository, blob_storage) = service();
        let key = StorageKey::new("assets/image.png").unwrap();
        let data = Bytes::from_static(b"image bytes");
        let checksum = Checksum::sha256("a".repeat(64)).unwrap();

        let error = block_on(service.upload_resource_content(
            UploadResourceContent::new("image", key.clone(), data).with_checksum(checksum),
        ))
        .unwrap_err();

        match error {
            CoreError::Conflict { message } => assert!(message.contains("sha256")),
            other => panic!("expected checksum conflict, got {other:?}"),
        }
        assert!(repository.is_empty());
        assert_eq!(blob_storage.get_sync(&key), None);
    }

    #[test]
    fn upload_resource_content_rejects_existing_storage_key() {
        let (service, repository, blob_storage) = service();
        let key = StorageKey::new("assets/image.png").unwrap();
        blob_storage
            .objects
            .lock()
            .unwrap()
            .insert(key.clone(), Bytes::from_static(b"existing"));

        let error = block_on(service.upload_resource_content(UploadResourceContent::new(
            "image",
            key,
            Bytes::from_static(b"new"),
        )))
        .unwrap_err();

        match error {
            CoreError::Conflict { message } => assert!(message.contains("already exists")),
            other => panic!("expected storage key conflict, got {other:?}"),
        }
        assert!(repository.is_empty());
    }

    #[test]
    fn create_resource_rejects_unsupported_kind() {
        let (service, repository, _) = service_with_registry(Arc::new(
            InMemoryResourceKindRegistry::with_definitions(vec![ResourceKindDefinition::new(
                ResourceKind::default(),
                "Unknown",
                None,
                true,
            )]),
        ));

        let error = block_on(
            service.create_resource(CreateResource::new("image").with_kind("plugin:not-installed")),
        )
        .unwrap_err();

        match error {
            CoreError::Configuration { message } => {
                assert!(message.contains("unsupported resource kind `plugin:not-installed`"))
            }
            other => panic!("expected configuration error, got {other:?}"),
        }
        assert!(repository.is_empty());
    }

    #[test]
    fn upload_resource_content_stream_writes_chunks_and_records_size() {
        let (service, repository, blob_storage) = service();
        let key = StorageKey::new("assets/large.bin").unwrap();
        let data: BlobByteStream = Box::pin(futures_util::stream::iter([
            Ok(Bytes::from_static(b"large ")),
            Ok(Bytes::from_static(b"file ")),
            Ok(Bytes::from_static(b"bytes")),
        ]));

        let resource = block_on(
            service.upload_resource_content_stream(
                UploadResourceContentStream::new("large file", key.clone(), data)
                    .with_kind("asset:binary")
                    .with_mime_type("application/octet-stream"),
            ),
        )
        .unwrap();

        let saved = repository.find_sync(&resource.id()).unwrap();
        let content = saved.content().unwrap();

        assert_eq!(content.key(), &key);
        assert_eq!(content.size(), 16);
        assert_eq!(content.mime_type(), Some("application/octet-stream"));
        assert_eq!(
            blob_storage.get_sync(&key),
            Some(Bytes::from_static(b"large file bytes"))
        );
    }

    #[test]
    fn upload_resource_content_rejects_kind_without_content_support() {
        let (service, repository, blob_storage) = service();
        let key = StorageKey::new("docs/readme.md").unwrap();

        let error = block_on(
            service.upload_resource_content(
                UploadResourceContent::new("readme", key.clone(), Bytes::from_static(b"hello"))
                    .with_kind("doc:markdown"),
            ),
        )
        .unwrap_err();

        match error {
            CoreError::Configuration { message } => {
                assert!(message.contains("does not support content upload"))
            }
            other => panic!("expected configuration error, got {other:?}"),
        }
        assert!(repository.is_empty());
        assert!(!blob_storage.contains(&key));
    }

    #[test]
    fn upload_resource_content_stream_removes_blob_on_checksum_mismatch() {
        let (service, repository, blob_storage) = service();
        let key = StorageKey::new("assets/large.bin").unwrap();
        let data: BlobByteStream = Box::pin(futures_util::stream::iter([
            Ok(Bytes::from_static(b"large ")),
            Ok(Bytes::from_static(b"file ")),
            Ok(Bytes::from_static(b"bytes")),
        ]));
        let checksum = Checksum::sha256("a".repeat(64)).unwrap();

        let error = block_on(
            service.upload_resource_content_stream(
                UploadResourceContentStream::new("large file", key.clone(), data)
                    .with_checksum(checksum),
            ),
        )
        .unwrap_err();

        match error {
            CoreError::Conflict { message } => assert!(message.contains("sha256")),
            other => panic!("expected checksum conflict, got {other:?}"),
        }
        assert!(repository.is_empty());
        assert!(!blob_storage.contains(&key));
    }

    #[test]
    fn upload_resource_content_removes_blob_when_save_fails() {
        let (service, repository, blob_storage) = service();
        let key = StorageKey::new("assets/image.png").unwrap();
        repository.fail_next_save();

        let result = block_on(service.upload_resource_content(UploadResourceContent::new(
            "image",
            key.clone(),
            Bytes::from_static(b"image bytes"),
        )));

        match result {
            Err(CoreError::Repository { operation, .. }) => assert_eq!(operation, "save"),
            other => panic!("expected repository error, got {other:?}"),
        }

        assert!(!blob_storage.contains(&key));
        assert!(repository.is_empty());
    }

    #[test]
    fn get_resource_content_reads_existing_blob() {
        let (service, _, _) = service();
        let key = StorageKey::new("assets/image.png").unwrap();
        let data = Bytes::from_static(b"image bytes");
        let resource = block_on(service.upload_resource_content(UploadResourceContent::new(
            "image",
            key,
            data.clone(),
        )))
        .unwrap();

        let content = block_on(service.get_resource_content(&resource.id())).unwrap();

        assert_eq!(content, Some(data));
    }

    #[test]
    fn read_resource_returns_text_for_reader_kind() {
        let (service, _, _) = service();
        let key = StorageKey::new("books/book.txt").unwrap();
        let resource = block_on(
            service.upload_resource_content(
                UploadResourceContent::new("book", key, Bytes::from_static(b"Hello book"))
                    .with_kind("core:document"),
            ),
        )
        .unwrap();

        let readable = block_on(service.read_resource(&resource.id()))
            .unwrap()
            .unwrap();

        assert_eq!(readable.kind().as_str(), "core:document");
        assert_eq!(
            readable.view(),
            &PluginView::Text(TextView {
                text: "Hello book".to_string()
            })
        );
    }

    #[test]
    fn execute_write_action_replaces_resource_content() {
        let (service, repository, blob_storage) = service();
        let key = StorageKey::new("docs/note.md").unwrap();
        let resource = block_on(
            service.upload_resource_content(
                UploadResourceContent::new("note.md", key.clone(), Bytes::from_static(b"# Old"))
                    .with_kind("core:document")
                    .with_mime_type("text/markdown")
                    .with_original_filename("note.md"),
            ),
        )
        .unwrap();

        let output = block_on(
            service.execute_resource_action(
                &resource.id(),
                ExecuteResourceAction::new("azvs.markdown.update")
                    .with_input(json!({"markdown": "# New\n\nUpdated."})),
            ),
        )
        .unwrap()
        .unwrap();

        assert_eq!(output.action().as_str(), "azvs.markdown.update");
        assert_eq!(
            blob_storage.get_sync(&key).unwrap(),
            Bytes::from_static(b"# New\n\nUpdated.")
        );
        let updated = repository.find_sync(&resource.id()).unwrap();
        let content = updated.content().unwrap();
        assert_eq!(content.size(), 15);
        assert_eq!(content.mime_type(), Some("text/markdown"));
        assert_eq!(content.original_filename(), Some("note.md"));
        assert_eq!(content.checksums().len(), 1);
        assert_eq!(content.checksums()[0].kind(), ChecksumKind::Sha256);
    }

    #[test]
    fn read_resource_rejects_non_reader_kind() {
        let (service, _, _) = service();
        let key = StorageKey::new("files/file.txt").unwrap();
        let resource = block_on(
            service.upload_resource_content(
                UploadResourceContent::new("file", key, Bytes::from_static(b"hello"))
                    .with_kind("asset:binary"),
            ),
        )
        .unwrap();

        let error = block_on(service.read_resource(&resource.id())).unwrap_err();

        match error {
            CoreError::Configuration { message } => {
                assert!(message.contains("does not support action `read`"))
            }
            other => panic!("expected configuration error, got {other:?}"),
        }
    }

    #[test]
    fn describe_resource_actions_uses_declared_actions_without_format_sniffing() {
        let (service, _, _) = service();
        let pdf = block_on(
            service.upload_resource_content(
                UploadResourceContent::new(
                    "book",
                    StorageKey::new("books/book.pdf").unwrap(),
                    Bytes::from_static(b"%PDF-1.4"),
                )
                .with_kind("core:document")
                .with_mime_type("application/pdf"),
            ),
        )
        .unwrap();
        let text = block_on(
            service.upload_resource_content(
                UploadResourceContent::new(
                    "book",
                    StorageKey::new("books/book.txt").unwrap(),
                    Bytes::from_static(b"hello"),
                )
                .with_kind("core:document")
                .with_mime_type("text/plain"),
            ),
        )
        .unwrap();

        let pdf_actions = service.describe_resource_actions(&pdf).unwrap();
        let text_actions = service.describe_resource_actions(&text).unwrap();

        assert!(pdf_actions.download_content());
        assert!(pdf_actions.read());
        assert!(!pdf_actions.view_inline());
        assert!(text_actions.download_content());
        assert!(text_actions.read());
        assert!(!text_actions.view_inline());
    }

    #[test]
    fn describe_resource_actions_filters_extension_actions_by_content_match() {
        let (service, _, _) = service();
        let mp4 = block_on(
            service.upload_resource_content(
                UploadResourceContent::new(
                    "demo.mp4",
                    StorageKey::new("videos/demo.mp4").unwrap(),
                    Bytes::from_static(b"mp4"),
                )
                .with_kind("core:video")
                .with_mime_type("video/mp4"),
            ),
        )
        .unwrap();
        let webm = block_on(
            service.upload_resource_content(
                UploadResourceContent::new(
                    "demo.webm",
                    StorageKey::new("videos/demo.webm").unwrap(),
                    Bytes::from_static(b"webm"),
                )
                .with_kind("core:video")
                .with_mime_type("video/webm"),
            ),
        )
        .unwrap();

        let mp4_actions = service.describe_resource_actions(&mp4).unwrap();
        let webm_actions = service.describe_resource_actions(&webm).unwrap();

        assert!(
            mp4_actions
                .available_actions()
                .iter()
                .any(|action| action.id().as_str() == "azvs.mp4.play")
        );
        assert!(
            !webm_actions
                .available_actions()
                .iter()
                .any(|action| action.id().as_str() == "azvs.mp4.play")
        );
    }

    #[test]
    fn preview_resource_returns_pdf_content_for_preview_kind() {
        let (service, _, _) = service();
        let resource = block_on(
            service.upload_resource_content(
                UploadResourceContent::new(
                    "book",
                    StorageKey::new("books/book.pdf").unwrap(),
                    Bytes::from_static(b"%PDF-1.4"),
                )
                .with_kind("core:document")
                .with_mime_type("application/pdf"),
            ),
        )
        .unwrap();

        let preview = block_on(service.preview_resource(&resource.id()))
            .unwrap()
            .unwrap();

        assert_eq!(preview.content_type(), "application/pdf");
        assert_eq!(preview.content().as_ref(), b"%PDF-1.4");
    }

    #[test]
    fn thumbnail_resource_returns_image_content_for_thumbnail_kind() {
        let (service, _, _) = service();
        let image = Bytes::from_static(b"fake-image");
        let resource = block_on(
            service.upload_resource_content(
                UploadResourceContent::new(
                    "image",
                    StorageKey::new("images/pixel.png").unwrap(),
                    image.clone(),
                )
                .with_kind("core:image")
                .with_mime_type("image/png"),
            ),
        )
        .unwrap();

        let thumbnail = block_on(service.thumbnail_resource(&resource.id()))
            .unwrap()
            .unwrap();

        assert_eq!(thumbnail.content_type(), "image/png");
        assert_eq!(thumbnail.content(), &image);
    }

    #[test]
    fn soft_delete_resource_keeps_blob_but_hides_content_read() {
        let (service, repository, blob_storage) = service();
        let key = StorageKey::new("assets/image.png").unwrap();
        let resource = block_on(service.upload_resource_content(UploadResourceContent::new(
            "image",
            key.clone(),
            Bytes::from_static(b"image bytes"),
        )))
        .unwrap();

        let deleted = block_on(service.soft_delete_resource(&resource.id()))
            .unwrap()
            .unwrap();
        let content = block_on(service.get_resource_content(&resource.id())).unwrap();

        assert!(deleted.is_deleted());
        assert!(repository.find_sync(&resource.id()).unwrap().is_deleted());
        assert!(blob_storage.contains(&key));
        assert!(content.is_none());
    }

    #[test]
    fn remove_resource_deletes_blob_and_repository_record() {
        let (service, repository, blob_storage) = service();
        let key = StorageKey::new("assets/image.png").unwrap();
        let resource = block_on(service.upload_resource_content(UploadResourceContent::new(
            "image",
            key.clone(),
            Bytes::from_static(b"image bytes"),
        )))
        .unwrap();

        assert!(block_on(service.remove_resource(&resource.id())).unwrap());
        assert!(repository.find_sync(&resource.id()).is_none());
        assert!(!blob_storage.contains(&key));
        assert!(!block_on(service.remove_resource(&resource.id())).unwrap());
    }
}
