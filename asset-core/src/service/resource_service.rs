//! 资源应用服务。
//!
//! 本模块提供围绕 `Resource` 聚合的应用用例。每个公开方法代表一个完整业务动作，
//! 负责把领域模型、资源仓储端口和对象存储端口编排在一起。
//!
//! 该层只定义业务流程，不绑定具体基础设施。OpenDAL、sqlx 等实现应通过 `port`
//! 模块中的 trait 注入进来。

use crate::CoreError;
use crate::domain::{
    Checksum, ChecksumKind, Resource, ResourceContent, ResourceDirectory, ResourceId, ResourceKind,
    ResourceMetadata, ResourceStatus, StorageKey,
};
use crate::port::{
    BlobByteStream, BlobStorage, ListResources, ResourceActionExecutor, ResourceActionOutput,
    ResourceActionRegistry, ResourceActionRequest, ResourceKindRegistry, ResourcePage,
    ResourceRepository, StorageScanner,
};
use asset_plugin_api::{PluginActionEffect, PluginContentEncoding, PluginView};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use bytes::Bytes;
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::sync::Arc;

const MAX_INLINE_PLUGIN_CONTENT_BYTES: u64 = 4 * 1024 * 1024;

mod action;
mod command;
mod content;
mod preview;
mod secured;

pub use action::ResourceActionService;
pub use command::ResourceCommandService;
pub use content::ResourceContentService;
pub use preview::ResourcePreviewService;
pub use secured::SecuredResourceService;

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
    /// 资源所在的逻辑目录。
    directory: ResourceDirectory,
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
            directory: ResourceDirectory::root(),
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

    /// 设置资源所在逻辑目录。
    pub fn with_directory(mut self, directory: ResourceDirectory) -> Self {
        self.directory = directory;
        self
    }

    /// 设置初始资源元数据。
    ///
    /// 未调用该方法时，资源元数据默认为服务端定义的空元数据结构。
    pub fn with_metadata(mut self, metadata: impl Into<ResourceMetadata>) -> Self {
        self.metadata = metadata.into();
        self
    }

    pub fn directory(&self) -> &ResourceDirectory {
        &self.directory
    }
}

/// 创建带内容资源的通用命令。公共资源和内容字段只在这里维护，`payload` 表示
/// 不同用例特有的输入，例如已存在对象的大小或待写入的字节流。
#[derive(Debug, Clone)]
pub struct ResourceContentCommand<T> {
    /// 资源展示名。
    name: String,
    /// 资源类型；未设置时会按内容特征自动推断。
    kind: Option<ResourceKind>,
    /// 初始生命周期状态。
    status: ResourceStatus,
    /// 资源所在的逻辑目录。
    directory: ResourceDirectory,
    /// 初始资源元数据。
    metadata: ResourceMetadata,
    /// 内容在对象存储中的定位键。
    storage_key: StorageKey,
    /// 用例特有的内容输入。
    payload: T,
    /// 内容 MIME 类型。
    mime_type: Option<String>,
    /// 原始文件名。
    original_filename: Option<String>,
    /// 内容校验和集合。
    checksums: Vec<Checksum>,
}

/// 导入已存在对象内容并创建资源；payload 是对象字节大小。
pub type ImportResourceContent = ResourceContentCommand<u64>;

/// 流式上传内容并创建资源；payload 是待写入对象存储的字节流。
pub type UploadResourceContentStream = ResourceContentCommand<BlobByteStream>;

/// 扫描对象存储并导入尚未登记资源的命令。
#[derive(Debug, Clone, Default)]
pub struct ScanStorage {
    directory: ResourceDirectory,
    include_sha256: bool,
}

impl ScanStorage {
    pub fn new(directory: ResourceDirectory) -> Self {
        Self {
            directory,
            include_sha256: false,
        }
    }

    pub fn with_sha256(mut self, include_sha256: bool) -> Self {
        self.include_sha256 = include_sha256;
        self
    }

    pub fn directory(&self) -> &ResourceDirectory {
        &self.directory
    }
}

#[derive(Debug, Clone)]
pub struct ScanStorageError {
    pub key: String,
    pub error: String,
}

#[derive(Debug, Clone)]
pub struct ScanStorageResult {
    pub scanned_directory: ResourceDirectory,
    pub scanned: u64,
    pub skipped: u64,
    pub errors: Vec<ScanStorageError>,
    pub resources: Vec<Resource>,
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

impl<T> ResourceContentCommand<T> {
    /// 创建命令，默认自动推断资源类型、使用活跃状态和空元数据。
    pub fn new(name: impl Into<String>, storage_key: StorageKey, payload: T) -> Self {
        Self {
            name: name.into(),
            kind: None,
            status: ResourceStatus::default(),
            directory: ResourceDirectory::root(),
            metadata: ResourceMetadata::default(),
            storage_key,
            payload,
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

    /// 设置资源所在逻辑目录。
    pub fn with_directory(mut self, directory: ResourceDirectory) -> Self {
        self.directory = directory;
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

    /// 设置原始文件名。
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

    pub fn directory(&self) -> &ResourceDirectory {
        &self.directory
    }
}

/// 更新资源的用例命令。
#[derive(Debug, Clone, Default)]
pub struct UpdateResource {
    /// 新资源名称。
    name: Option<String>,
    /// 新逻辑目录。
    directory: Option<ResourceDirectory>,
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

    /// 设置新逻辑目录。
    pub fn with_directory(mut self, directory: ResourceDirectory) -> Self {
        self.directory = Some(directory);
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
    available_actions: Vec<crate::port::ResourceActionDefinition>,
}

impl ResourceActions {
    fn new(available_actions: Vec<crate::port::ResourceActionDefinition>) -> Self {
        Self { available_actions }
    }

    /// 返回当前资源可执行的全部动作。
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

/// 流式读取资源内容结果。
pub struct ResourceContentStream {
    content_type: String,
    content_length: u64,
    content: BlobByteStream,
}

impl ResourceContentStream {
    fn new(content_type: String, content_length: u64, content: BlobByteStream) -> Self {
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
/// 具体用例按职责拆分到命令、内容、动作和预览服务中，调用方通过对应的子服务访问能力。
///
/// 对象存储和数据库之间没有分布式事务。本服务会在关键流程中做必要的顺序控制和
/// 最小补偿，但调用方仍应根据业务需要在更外层增加重试、任务补偿或审计机制。
#[derive(Clone)]
pub struct ResourceService {
    /// 资源聚合仓储端口。
    repository: Arc<dyn ResourceRepository>,
    /// 对象存储端口。
    blob_storage: Arc<dyn BlobStorage>,
    storage_scanner: Arc<dyn StorageScanner>,
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
        storage_scanner: Arc<dyn StorageScanner>,
        kind_registry: Arc<dyn ResourceKindRegistry>,
    ) -> Self {
        Self {
            repository,
            blob_storage,
            storage_scanner,
            kind_registry,
            action_registry: None,
            action_executor: None,
        }
    }

    /// 创建带资源动作执行器的资源应用服务。
    pub fn new_with_action_executor(
        repository: Arc<dyn ResourceRepository>,
        blob_storage: Arc<dyn BlobStorage>,
        storage_scanner: Arc<dyn StorageScanner>,
        kind_registry: Arc<dyn ResourceKindRegistry>,
        action_executor: Arc<dyn ResourceActionExecutor>,
    ) -> Self {
        Self {
            repository,
            blob_storage,
            storage_scanner,
            kind_registry,
            action_registry: None,
            action_executor: Some(action_executor),
        }
    }

    /// 创建带全局资源动作注册表和动作执行器的资源应用服务。
    pub fn new_with_action_registry_and_executor(
        repository: Arc<dyn ResourceRepository>,
        blob_storage: Arc<dyn BlobStorage>,
        storage_scanner: Arc<dyn StorageScanner>,
        kind_registry: Arc<dyn ResourceKindRegistry>,
        action_registry: Arc<dyn ResourceActionRegistry>,
        action_executor: Arc<dyn ResourceActionExecutor>,
    ) -> Self {
        Self {
            repository,
            blob_storage,
            storage_scanner,
            kind_registry,
            action_registry: Some(action_registry),
            action_executor: Some(action_executor),
        }
    }
    /// 返回资源命令服务。
    ///
    /// 命令服务负责资源聚合本身的生命周期变化，例如创建、更新、软删除和物理移除。
    pub fn commands(&self) -> ResourceCommandService<'_> {
        ResourceCommandService::new(self)
    }

    /// 返回资源内容服务。
    ///
    /// 内容服务负责对象内容的上传、流式上传和下载，并处理对象存储与资源仓储之间的补偿。
    pub fn content(&self) -> ResourceContentService<'_> {
        ResourceContentService::new(self)
    }

    /// 返回资源动作服务。
    ///
    /// 动作服务负责解析 kind/action 声明、执行动作和应用动作返回的写入效果。
    pub fn actions(&self) -> ResourceActionService<'_> {
        ResourceActionService::new(self)
    }

    /// 返回资源预览服务。
    ///
    /// 预览服务负责 read、preview、thumbnail 等面向展示的读取流程。
    pub fn previews(&self) -> ResourcePreviewService<'_> {
        ResourcePreviewService::new(self)
    }

    /// 将资源用例绑定到访问主体。HTTP、CLI、TUI 等非可信入口应通过该门面执行资源操作。
    pub fn secured<'a>(
        &'a self,
        authorization: &'a crate::service::AuthorizationService,
        context: &'a crate::domain::AccessContext,
    ) -> SecuredResourceService<'a> {
        SecuredResourceService::new(self, authorization, context)
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
    ) -> Result<&crate::port::ResourceKindDefinition, CoreError> {
        self.kind_registry
            .get(kind)
            .ok_or_else(|| CoreError::configuration(format!("unsupported resource kind `{kind}`")))
    }

    fn actions_for_resource_kind(
        &self,
        kind: &ResourceKind,
    ) -> Vec<crate::port::ResourceActionDefinition> {
        let lineage = self.kind_registry.lineage(kind);
        let mut actions = self.kind_registry.actions_for_kind(kind);
        if let Some(registry) = &self.action_registry {
            for action in registry.actions_for_kinds(&lineage) {
                if !actions.iter().any(|existing| existing.id() == action.id()) {
                    actions.push(action);
                }
            }
        }
        actions
    }

    fn action_matches_resource(
        &self,
        action: &crate::port::ResourceActionDefinition,
        resource: &Resource,
    ) -> bool {
        let content = resource.content();
        self.kind_registry
            .lineage(resource.kind())
            .iter()
            .any(|kind| {
                action.matches_resource(
                    kind.as_str(),
                    content.and_then(|content| content.mime_type()),
                    content.map(|content| content.key().as_str()),
                )
            })
    }
}

fn resolved_content_delivery(
    action: &crate::port::ResourceActionDefinition,
    size: u64,
) -> Option<crate::port::ResourceActionContentDelivery> {
    if !action.requirements().content {
        return None;
    }
    match action.requirements().content_delivery {
        crate::port::ResourceActionContentDelivery::Auto
            if size <= MAX_INLINE_PLUGIN_CONTENT_BYTES =>
        {
            Some(crate::port::ResourceActionContentDelivery::Inline)
        }
        crate::port::ResourceActionContentDelivery::Auto => {
            Some(crate::port::ResourceActionContentDelivery::Url)
        }
        delivery => Some(delivery),
    }
}

fn should_load_declared_action_content(
    action: &crate::port::ResourceActionDefinition,
    content: &ResourceContent,
) -> bool {
    matches!(
        resolved_content_delivery(action, content.size()),
        Some(crate::port::ResourceActionContentDelivery::Inline)
    )
}

fn decode_media_view(action: &str, view: &PluginView) -> Result<(String, Bytes), CoreError> {
    let PluginView::Media(media) = view else {
        return Err(CoreError::configuration(format!(
            "resource action `{action}` must return a media view"
        )));
    };
    if media.encoding != PluginContentEncoding::Base64 {
        return Err(CoreError::configuration(format!(
            "resource action `{action}` returned unsupported media encoding"
        )));
    }
    let content = BASE64_STANDARD.decode(&media.data).map_err(|error| {
        CoreError::configuration(format!(
            "resource action `{action}` returned invalid media: {error}"
        ))
    })?;

    Ok((media.mime_type.clone(), Bytes::from(content)))
}

fn content_type_for_media(content: &ResourceContent) -> String {
    content
        .mime_type()
        .unwrap_or("application/octet-stream")
        .to_string()
}

fn build_resource(
    name: String,
    directory: ResourceDirectory,
    kind: Option<ResourceKind>,
    status: ResourceStatus,
    metadata: ResourceMetadata,
) -> crate::domain::ResourceBuilder {
    let mut builder = Resource::builder(name)
        .with_directory(directory)
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
) -> (BlobByteStream, Arc<std::sync::Mutex<Sha256>>) {
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

    (Box::pin(stream), state)
}

fn finalize_tracked_checksum(
    sha256_state: Arc<std::sync::Mutex<Sha256>>,
    checksums: &[Checksum],
) -> Result<Checksum, CoreError> {
    let digest = sha256_state
        .lock()
        .expect("sha256 mutex should not be poisoned")
        .clone()
        .finalize();
    let actual = hex_digest(&digest);

    if let Some(expected) = sha256_checksum(checksums)
        && !actual.eq_ignore_ascii_case(expected.value())
    {
        return Err(CoreError::conflict("sha256 checksum mismatch"));
    }

    Checksum::sha256(actual).map_err(Into::into)
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

        async fn find_by_content_key(
            &self,
            key: &StorageKey,
        ) -> Result<Option<Resource>, CoreError> {
            Ok(self
                .resources
                .lock()
                .unwrap()
                .values()
                .find(|resource| {
                    resource
                        .content()
                        .is_some_and(|content| content.key().as_str() == key.as_str())
                })
                .cloned())
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
                .filter(|resource| {
                    query
                        .directory()
                        .is_none_or(|directory| resource.directory() == directory)
                })
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

        async fn list_directories(
            &self,
            parent: &ResourceDirectory,
        ) -> Result<Vec<ResourceDirectory>, CoreError> {
            let mut directories = self
                .resources
                .lock()
                .unwrap()
                .values()
                .filter_map(|resource| child_directory(resource.directory(), parent))
                .collect::<Vec<_>>();
            directories.sort_by(|left, right| left.path().cmp(right.path()));
            directories.dedup_by(|left, right| left.path() == right.path());
            Ok(directories)
        }
    }

    fn child_directory(
        directory: &ResourceDirectory,
        parent: &ResourceDirectory,
    ) -> Option<ResourceDirectory> {
        if directory.is_root() {
            return None;
        }
        let directory = directory.path();
        let remainder = if parent.is_root() {
            directory
        } else {
            directory.strip_prefix(parent.path())?.strip_prefix('/')?
        };
        parent.child(remainder.split('/').next()?).ok()
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
        fn definitions(&self) -> &[ResourceKindDefinition] {
            &self.definitions
        }
    }

    #[derive(Default)]
    struct InMemoryBlobStorage {
        objects: Mutex<HashMap<StorageKey, Bytes>>,
        fail_next_delete: Mutex<bool>,
    }

    impl InMemoryBlobStorage {
        fn contains(&self, key: &StorageKey) -> bool {
            self.objects.lock().unwrap().contains_key(key)
        }

        fn get_sync(&self, key: &StorageKey) -> Option<Bytes> {
            self.objects.lock().unwrap().get(key).cloned()
        }

        fn contains_fragment(&self, fragment: &str) -> bool {
            self.objects
                .lock()
                .unwrap()
                .keys()
                .any(|key| key.as_str().contains(fragment))
        }

        fn fail_next_delete(&self) {
            *self.fail_next_delete.lock().unwrap() = true;
        }
    }

    #[async_trait::async_trait]
    impl BlobStorage for InMemoryBlobStorage {
        async fn put(&self, key: &StorageKey, data: Bytes) -> Result<(), CoreError> {
            self.objects.lock().unwrap().insert(key.clone(), data);
            Ok(())
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

        async fn get_range_stream(
            &self,
            key: &StorageKey,
            start: u64,
            end: u64,
        ) -> Result<Option<BlobByteStream>, CoreError> {
            Ok(self.get_sync(key).map(|content| {
                let content = content.slice(start as usize..end as usize + 1);
                Box::pin(futures_util::stream::once(async move { Ok(content) })) as BlobByteStream
            }))
        }

        async fn delete(&self, key: &StorageKey) -> Result<(), CoreError> {
            if std::mem::take(&mut *self.fail_next_delete.lock().unwrap()) {
                return Err(CoreError::storage("delete", TestError("delete failed")));
            }
            self.objects.lock().unwrap().remove(key);
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl StorageScanner for InMemoryBlobStorage {
        async fn scan(
            &self,
            _directory: &ResourceDirectory,
            _include_sha256: bool,
            _max_entries: usize,
        ) -> Result<Vec<crate::port::ScannedBlob>, CoreError> {
            Ok(Vec::new())
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
            ResourceKindDefinition::new(ResourceKind::default(), "Unknown", true),
            ResourceKindDefinition::new(ResourceKind::try_new("core:file").unwrap(), "File", true),
            ResourceKindDefinition::new(
                ResourceKind::try_new("doc:markdown").unwrap(),
                "Markdown",
                false,
            )
            .with_actions(vec![
                ResourceActionDefinition::new(ResourceAction::READ, "Read")
                    .with_handler("read_markdown")
                    .with_requirements(content_requirements())
                    .with_output(output_contract(["text"])),
                ResourceActionDefinition::new("metadata.inspect", "Inspect metadata")
                    .with_handler("inspect_metadata")
                    .with_output(output_contract(["json"])),
            ]),
            ResourceKindDefinition::new(
                ResourceKind::try_new("core:image").unwrap(),
                "Image",
                true,
            )
            .with_actions(vec![
                ResourceActionDefinition::new(ResourceAction::PREVIEW, "Preview")
                    .with_handler("preview_image")
                    .with_requirements(content_requirements())
                    .with_output(output_contract(["media"])),
                ResourceActionDefinition::new(ResourceAction::THUMBNAIL, "Thumbnail")
                    .with_handler("thumbnail_image")
                    .with_requirements(content_requirements())
                    .with_output(output_contract(["media"])),
            ]),
            ResourceKindDefinition::new(
                ResourceKind::try_new("asset:binary").unwrap(),
                "Binary",
                true,
            ),
            ResourceKindDefinition::new(
                ResourceKind::try_new("core:document").unwrap(),
                "Document",
                true,
            )
            .with_actions(vec![
                ResourceActionDefinition::new(ResourceAction::READ, "Read")
                    .with_handler("read_document")
                    .with_requirements(content_requirements())
                    .with_output(output_contract(["text"])),
                ResourceActionDefinition::new("azvs.markdown.render", "Read Markdown")
                    .with_handler("render_markdown")
                    .with_requirements(content_requirements())
                    .with_output(output_contract(["plugin_frame"]))
                    .with_content_matcher(
                        crate::port::ResourceContentMatcher::new()
                            .with_mime_types(["text/markdown", "text/x-markdown"])
                            .with_extensions([".md", ".markdown"]),
                    ),
                ResourceActionDefinition::new("azvs.markdown.update", "Edit Markdown")
                    .with_handler("update_markdown")
                    .with_requirements(content_requirements())
                    .with_access(crate::port::ResourceActionAccess::ReadWrite)
                    .with_output(output_contract(["text"]))
                    .with_content_matcher(
                        crate::port::ResourceContentMatcher::new()
                            .with_mime_types(["text/markdown", "text/x-markdown"])
                            .with_extensions([".md", ".markdown"]),
                    ),
                ResourceActionDefinition::new(ResourceAction::PREVIEW, "Preview")
                    .with_handler("preview_document")
                    .with_requirements(content_requirements())
                    .with_output(output_contract(["media"])),
            ]),
            ResourceKindDefinition::new(
                ResourceKind::try_new("azvs:markdown").unwrap(),
                "Markdown Document",
                true,
            )
            .with_parent(Some(ResourceKind::try_new("core:document").unwrap()))
            .with_detect(
                crate::port::ResourceContentMatcher::new()
                    .with_mime_types(["text/markdown", "text/x-markdown"])
                    .with_extensions([".md", ".markdown"]),
            ),
            ResourceKindDefinition::new(
                ResourceKind::try_new("core:video").unwrap(),
                "Video",
                true,
            )
            .with_actions(vec![ResourceActionDefinition::new(
                ResourceAction::PREVIEW,
                "Preview",
            )]),
        ]));
        let repository = Arc::new(InMemoryResourceRepository::default());
        let blob_storage = Arc::new(InMemoryBlobStorage::default());
        let service = ResourceService::new_with_action_executor(
            repository.clone(),
            blob_storage.clone(),
            blob_storage.clone(),
            kind_registry,
            Arc::new(StaticResourceActionExecutor),
        );

        (service, repository, blob_storage)
    }

    fn content_requirements() -> asset_plugin_api::ResourceActionRequirements {
        asset_plugin_api::ResourceActionRequirements {
            content: true,
            content_delivery: asset_plugin_api::ResourceActionContentDelivery::Inline,
        }
    }

    fn output_contract<const N: usize>(
        views: [&str; N],
    ) -> asset_plugin_api::ResourceActionOutputContract {
        asset_plugin_api::ResourceActionOutputContract {
            view: views.into_iter().map(str::to_string).collect(),
        }
    }

    fn stream_upload_command(
        name: impl Into<String>,
        storage_key: StorageKey,
        data: Bytes,
    ) -> UploadResourceContentStream {
        let stream = futures_util::stream::once(async move { Ok(data) });
        UploadResourceContentStream::new(name, storage_key, Box::pin(stream))
    }

    #[test]
    fn action_content_delivery_never_loads_unrequested_content() {
        use asset_plugin_api::{ResourceActionContentDelivery, ResourceActionRequirements};

        let without_content = ResourceActionDefinition::new("inspect", "Inspect");
        assert_eq!(resolved_content_delivery(&without_content, 1), None);

        let required = |delivery| {
            ResourceActionDefinition::new("inspect", "Inspect").with_requirements(
                ResourceActionRequirements {
                    content: true,
                    content_delivery: delivery,
                },
            )
        };
        assert_eq!(
            resolved_content_delivery(
                &required(ResourceActionContentDelivery::Inline),
                MAX_INLINE_PLUGIN_CONTENT_BYTES + 1,
            ),
            Some(ResourceActionContentDelivery::Inline)
        );
        assert_eq!(
            resolved_content_delivery(&required(ResourceActionContentDelivery::Url), 1,),
            Some(ResourceActionContentDelivery::Url)
        );
        assert_eq!(
            resolved_content_delivery(
                &required(ResourceActionContentDelivery::Auto),
                MAX_INLINE_PLUGIN_CONTENT_BYTES,
            ),
            Some(ResourceActionContentDelivery::Inline)
        );
        assert_eq!(
            resolved_content_delivery(
                &required(ResourceActionContentDelivery::Auto),
                MAX_INLINE_PLUGIN_CONTENT_BYTES + 1,
            ),
            Some(ResourceActionContentDelivery::Url)
        );
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
        let service = ResourceService::new(
            repository.clone(),
            blob_storage.clone(),
            blob_storage.clone(),
            kind_registry,
        );

        (service, repository, blob_storage)
    }

    #[test]
    fn create_resource_saves_metadata_only_resource() {
        let (service, repository, _) = service();
        let metadata = ResourceMetadata::builder()
            .with_description(" Design document ")
            .with_tags(["rust", "asset"])
            .build()
            .unwrap();

        let resource = block_on(
            service.commands().create_resource(
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
    }

    #[test]
    fn metadata_only_resource_describes_only_actions_without_content_requirements() {
        let (service, _, _) = service();
        let resource = block_on(
            service
                .commands()
                .create_resource(CreateResource::new("metadata").with_kind("doc:markdown")),
        )
        .unwrap();

        let actions = service
            .actions()
            .describe_resource_actions(&resource)
            .unwrap();
        let ids = actions
            .available_actions()
            .iter()
            .map(|action| action.id().as_str())
            .collect::<Vec<_>>();

        assert_eq!(ids, vec!["metadata.inspect"]);
    }

    #[test]
    fn metadata_only_resource_rejects_direct_content_action_execution() {
        let (service, _, _) = service();
        let resource = block_on(
            service
                .commands()
                .create_resource(CreateResource::new("metadata").with_kind("doc:markdown")),
        )
        .unwrap();

        let error = block_on(service.actions().execute_resource_action(
            &resource.id(),
            ExecuteResourceAction::new(ResourceAction::READ),
        ))
        .unwrap_err();

        assert!(error.to_string().contains("does not support action `read`"));
    }

    #[test]
    fn stream_upload_resource_content_writes_blob_then_saves_resource() {
        let (service, repository, blob_storage) = service();
        let key = StorageKey::new("assets/image.png").unwrap();
        let data = Bytes::from_static(b"image bytes");
        let checksum = Checksum::sha256(hex_sha256(&data)).unwrap();

        let resource = block_on(
            service.content().upload_resource_content_stream(
                stream_upload_command("image", key.clone(), data.clone())
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
        assert_eq!(content.checksums().collect::<Vec<_>>(), vec![&checksum]);
        assert_eq!(blob_storage.get_sync(&key), Some(data));
    }

    #[test]
    fn stream_upload_resource_content_detects_most_specific_kind() {
        let (service, repository, _) = service();
        let key = StorageKey::new("docs/readme.md").unwrap();

        let resource = block_on(
            service.content().upload_resource_content_stream(
                stream_upload_command("readme", key, Bytes::from_static(b"# Readme"))
                    .with_mime_type("text/plain")
                    .with_original_filename("README.md"),
            ),
        )
        .unwrap();

        let saved = repository.find_sync(&resource.id()).unwrap();

        assert!(saved.kind().is("azvs:markdown"));
    }

    #[test]
    fn stream_upload_resource_content_rejects_checksum_mismatch() {
        let (service, repository, blob_storage) = service();
        let key = StorageKey::new("assets/image.png").unwrap();
        let data = Bytes::from_static(b"image bytes");
        let checksum = Checksum::sha256("a".repeat(64)).unwrap();

        let error = block_on(service.content().upload_resource_content_stream(
            stream_upload_command("image", key.clone(), data).with_checksum(checksum),
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
    fn stream_upload_resource_content_rejects_existing_storage_key() {
        let (service, repository, blob_storage) = service();
        let key = StorageKey::new("assets/image.png").unwrap();
        blob_storage
            .objects
            .lock()
            .unwrap()
            .insert(key.clone(), Bytes::from_static(b"existing"));

        let error = block_on(service.content().upload_resource_content_stream(
            stream_upload_command("image", key, Bytes::from_static(b"new")),
        ))
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
                true,
            )]),
        ));

        let error = block_on(
            service
                .commands()
                .create_resource(CreateResource::new("image").with_kind("plugin:not-installed")),
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
    fn stream_upload_resource_content_stream_writes_chunks_and_records_size() {
        let (service, repository, blob_storage) = service();
        let key = StorageKey::new("assets/large.bin").unwrap();
        let data: BlobByteStream = Box::pin(futures_util::stream::iter([
            Ok(Bytes::from_static(b"large ")),
            Ok(Bytes::from_static(b"file ")),
            Ok(Bytes::from_static(b"bytes")),
        ]));

        let resource = block_on(
            service.content().upload_resource_content_stream(
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
    fn stream_upload_resource_content_rejects_kind_without_content_support() {
        let (service, repository, blob_storage) = service();
        let key = StorageKey::new("docs/readme.md").unwrap();

        let error = block_on(
            service.content().upload_resource_content_stream(
                stream_upload_command("readme", key.clone(), Bytes::from_static(b"hello"))
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
    fn stream_upload_resource_content_stream_removes_blob_on_checksum_mismatch() {
        let (service, repository, blob_storage) = service();
        let key = StorageKey::new("assets/large.bin").unwrap();
        let data: BlobByteStream = Box::pin(futures_util::stream::iter([
            Ok(Bytes::from_static(b"large ")),
            Ok(Bytes::from_static(b"file ")),
            Ok(Bytes::from_static(b"bytes")),
        ]));
        let checksum = Checksum::sha256("a".repeat(64)).unwrap();

        let error = block_on(
            service.content().upload_resource_content_stream(
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
    fn stream_upload_resource_content_removes_blob_when_save_fails() {
        let (service, repository, blob_storage) = service();
        let key = StorageKey::new("assets/image.png").unwrap();
        repository.fail_next_save();

        let result = block_on(service.content().upload_resource_content_stream(
            stream_upload_command("image", key.clone(), Bytes::from_static(b"image bytes")),
        ));

        match result {
            Err(CoreError::Repository { operation, .. }) => assert_eq!(operation, "save"),
            other => panic!("expected repository error, got {other:?}"),
        }

        assert!(!blob_storage.contains(&key));
        assert!(repository.is_empty());
    }

    #[test]
    fn upload_preserves_repository_error_when_compensation_delete_fails() {
        let (service, repository, blob_storage) = service();
        let key = StorageKey::new("assets/compensation.bin").unwrap();
        repository.fail_next_save();
        blob_storage.fail_next_delete();

        let error = block_on(service.content().upload_resource_content_stream(
            stream_upload_command("file", key.clone(), Bytes::from_static(b"data")),
        ))
        .unwrap_err();

        assert!(matches!(
            error,
            CoreError::Repository {
                operation: "save",
                ..
            }
        ));
        assert!(blob_storage.contains(&key));
    }

    #[test]
    fn get_resource_content_reads_existing_blob() {
        let (service, _, _) = service();
        let key = StorageKey::new("assets/image.png").unwrap();
        let data = Bytes::from_static(b"image bytes");
        let resource = block_on(
            service
                .content()
                .upload_resource_content_stream(stream_upload_command("image", key, data.clone())),
        )
        .unwrap();

        let content = block_on(service.content().get_resource_content(&resource.id())).unwrap();

        assert_eq!(content, Some(data));
    }

    #[test]
    fn read_resource_returns_text_for_reader_kind() {
        let (service, _, _) = service();
        let key = StorageKey::new("books/book.txt").unwrap();
        let resource = block_on(
            service.content().upload_resource_content_stream(
                stream_upload_command("book", key, Bytes::from_static(b"Hello book"))
                    .with_kind("core:document"),
            ),
        )
        .unwrap();

        let readable = block_on(service.previews().read_resource(&resource.id()))
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
            service.content().upload_resource_content_stream(
                stream_upload_command("note.md", key.clone(), Bytes::from_static(b"# Old"))
                    .with_kind("core:document")
                    .with_mime_type("text/markdown")
                    .with_original_filename("note.md"),
            ),
        )
        .unwrap();

        let output = block_on(
            service.actions().execute_resource_action(
                &resource.id(),
                ExecuteResourceAction::new("azvs.markdown.update")
                    .with_input(json!({"markdown": "# New\n\nUpdated."})),
            ),
        )
        .unwrap()
        .unwrap();

        assert_eq!(output.action().as_str(), "azvs.markdown.update");
        let updated = repository.find_sync(&resource.id()).unwrap();
        let content = updated.content().unwrap();
        assert!(blob_storage.contains(&key));
        assert_eq!(content.key(), &key);
        assert!(!blob_storage.contains_fragment(".action-replacements/"));
        assert!(!blob_storage.contains_fragment(".action-backups/"));
        assert_eq!(
            blob_storage.get_sync(content.key()).unwrap(),
            Bytes::from_static(b"# New\n\nUpdated.")
        );
        assert_eq!(content.size(), 15);
        assert_eq!(content.mime_type(), Some("text/markdown"));
        assert_eq!(content.original_filename(), Some("note.md"));
        let checksums = content.checksums().collect::<Vec<_>>();
        assert_eq!(checksums.len(), 1);
        assert_eq!(checksums[0].kind(), ChecksumKind::Sha256);
    }

    #[test]
    fn read_resource_rejects_non_reader_kind() {
        let (service, _, _) = service();
        let key = StorageKey::new("files/file.txt").unwrap();
        let resource = block_on(
            service.content().upload_resource_content_stream(
                stream_upload_command("file", key, Bytes::from_static(b"hello"))
                    .with_kind("asset:binary"),
            ),
        )
        .unwrap();

        let error = block_on(service.previews().read_resource(&resource.id())).unwrap_err();

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
            service.content().upload_resource_content_stream(
                stream_upload_command(
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
            service.content().upload_resource_content_stream(
                stream_upload_command(
                    "book",
                    StorageKey::new("books/book.txt").unwrap(),
                    Bytes::from_static(b"hello"),
                )
                .with_kind("core:document")
                .with_mime_type("text/plain"),
            ),
        )
        .unwrap();

        let pdf_actions = service.actions().describe_resource_actions(&pdf).unwrap();
        let text_actions = service.actions().describe_resource_actions(&text).unwrap();
        let has_action = |actions: &ResourceActions, id: &str| {
            actions
                .available_actions()
                .iter()
                .any(|action| action.id().as_str() == id)
        };

        assert!(has_action(&pdf_actions, "download_content"));
        assert!(has_action(&pdf_actions, "read"));
        assert!(!has_action(&pdf_actions, "view_inline"));
        assert!(has_action(&text_actions, "download_content"));
        assert!(has_action(&text_actions, "read"));
        assert!(!has_action(&text_actions, "view_inline"));
    }

    #[test]
    fn core_video_resources_use_builtin_preview_for_common_video_formats() {
        let (service, _, _) = service();
        let mp4 = block_on(
            service.content().upload_resource_content_stream(
                stream_upload_command(
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
            service.content().upload_resource_content_stream(
                stream_upload_command(
                    "demo.webm",
                    StorageKey::new("videos/demo.webm").unwrap(),
                    Bytes::from_static(b"webm"),
                )
                .with_kind("core:video")
                .with_mime_type("video/webm"),
            ),
        )
        .unwrap();

        let mp4_actions = service.actions().describe_resource_actions(&mp4).unwrap();
        let webm_actions = service.actions().describe_resource_actions(&webm).unwrap();

        assert!(
            mp4_actions
                .available_actions()
                .iter()
                .any(|action| action.id().as_str() == ResourceAction::PREVIEW)
        );
        assert!(
            webm_actions
                .available_actions()
                .iter()
                .any(|action| action.id().as_str() == ResourceAction::PREVIEW)
        );
    }

    #[test]
    fn preview_resource_returns_pdf_content_for_preview_kind() {
        let (service, _, _) = service();
        let resource = block_on(
            service.content().upload_resource_content_stream(
                stream_upload_command(
                    "book",
                    StorageKey::new("books/book.pdf").unwrap(),
                    Bytes::from_static(b"%PDF-1.4"),
                )
                .with_kind("core:document")
                .with_mime_type("application/pdf"),
            ),
        )
        .unwrap();

        let preview = block_on(service.previews().preview_resource(&resource.id()))
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
            service.content().upload_resource_content_stream(
                stream_upload_command(
                    "image",
                    StorageKey::new("images/pixel.png").unwrap(),
                    image.clone(),
                )
                .with_kind("core:image")
                .with_mime_type("image/png"),
            ),
        )
        .unwrap();

        let thumbnail = block_on(service.previews().thumbnail_resource(&resource.id()))
            .unwrap()
            .unwrap();

        assert_eq!(thumbnail.content_type(), "image/png");
        assert_eq!(thumbnail.content(), &image);
    }

    #[test]
    fn soft_delete_resource_keeps_blob_but_hides_content_read() {
        let (service, repository, blob_storage) = service();
        let key = StorageKey::new("assets/image.png").unwrap();
        let resource = block_on(service.content().upload_resource_content_stream(
            stream_upload_command("image", key.clone(), Bytes::from_static(b"image bytes")),
        ))
        .unwrap();

        let deleted = block_on(service.commands().soft_delete_resource(&resource.id()))
            .unwrap()
            .unwrap();
        let content = block_on(service.content().get_resource_content(&resource.id())).unwrap();

        assert!(deleted.is_deleted());
        assert!(repository.find_sync(&resource.id()).unwrap().is_deleted());
        assert!(blob_storage.contains(&key));
        assert!(content.is_none());
    }

    #[test]
    fn remove_resource_deletes_blob_and_repository_record() {
        let (service, repository, blob_storage) = service();
        let key = StorageKey::new("assets/image.png").unwrap();
        let resource = block_on(service.content().upload_resource_content_stream(
            stream_upload_command("image", key.clone(), Bytes::from_static(b"image bytes")),
        ))
        .unwrap();

        assert!(block_on(service.commands().remove_resource(&resource.id())).unwrap());
        assert!(repository.find_sync(&resource.id()).is_none());
        assert!(!blob_storage.contains(&key));
        assert!(!block_on(service.commands().remove_resource(&resource.id())).unwrap());
    }
}
