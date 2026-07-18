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
    ResourceStatus, StorageKey,
};
use crate::port::{
    BlobByteStream, BlobStorage, DirectoryStorage, ListResources, RESERVED_BLOB_STORAGE_PREFIX,
    ResourceActionExecutor, ResourceActionOutput, ResourceActionRegistry, ResourceActionRequest,
    ResourceKindRegistry, ResourcePage, ResourceQuery, ResourceRepository, StoragePrefix,
    StorageScanner,
};
use asset_plugin_api::{
    PluginActionEffect, PluginExecutionPolicy, PluginMediaEncoding, PluginView, ResourceAction,
    ResourceActionAccess, ResourceActionContentDelivery, ResourceActionDefinition,
    ResourceActionExecutorKind,
};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use bytes::Bytes;
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::sync::Arc;

mod action;
mod command;
mod content;
mod preview;
mod secured;

use action::ResourceActionService;
use command::ResourceCommandService;
use content::ResourceContentService;
use preview::ResourcePreviewService;
pub use secured::SecuredResourceService;

/// 新内容使用的校验算法。
///
/// 这是服务端实现策略，不暴露给调用方。未来替换算法时在这里切换，并为旧算法保留读取支持。
const CONTENT_CHECKSUM_KIND: ChecksumKind = ChecksumKind::Sha256;

/// 创建不包含对象内容的资源用例命令。
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
    /// 初始资源描述。
    description: Option<String>,
    /// 初始资源标签。
    tags: Vec<String>,
}

impl CreateResource {
    /// 创建命令，默认自动推断资源类型、使用活跃状态、空描述和空标签。
    ///
    /// `name` 会在 usecase 执行时去除首尾空白并校验，不会在命令构造阶段提前校验。
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

    /// 设置初始资源描述。
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// 设置初始资源标签。
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
    /// 初始资源描述。
    description: Option<String>,
    /// 初始资源标签。
    tags: Vec<String>,
    /// 用例特有的内容输入。
    payload: T,
    /// 内容 MIME 类型。
    mime_type: Option<String>,
}

/// 导入已存在对象内容并创建资源；payload 是对象字节大小。
pub type ImportResourceContent = ResourceContentCommand<u64>;

/// 流式上传内容并创建资源；payload 是待写入对象存储的字节流。
pub type UploadResourceContentStream = ResourceContentCommand<BlobByteStream>;

/// 扫描对象存储并导入尚未登记资源的命令。
#[derive(Debug, Clone, Default)]
pub struct ScanStorage {
    prefix: StoragePrefix,
}

impl ScanStorage {
    pub fn new(prefix: StoragePrefix) -> Self {
        Self { prefix }
    }

    pub fn prefix(&self) -> &StoragePrefix {
        &self.prefix
    }
}

#[derive(Debug, Clone)]
pub struct ScanStorageError {
    pub key: String,
    pub error: String,
}

#[derive(Debug, Clone)]
pub struct ScanStorageResult {
    pub scanned_prefix: StoragePrefix,
    pub scanned: u64,
    pub skipped: u64,
    pub errors: Vec<ScanStorageError>,
    pub resources: Vec<Resource>,
}

/// 执行资源动作的用例命令。
#[derive(Debug, Clone)]
pub struct ExecuteResourceAction {
    action: ResourceAction,
    input: serde_json::Value,
}

impl ExecuteResourceAction {
    /// 创建资源动作执行命令。
    pub fn new(action: impl Into<ResourceAction>) -> Self {
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
    /// 创建命令，默认自动推断资源类型、使用活跃状态、空描述和空标签。
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

    /// 设置初始资源描述。
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// 设置初始资源标签。
    pub fn with_tags<U, I>(mut self, tags: I) -> Self
    where
        U: Into<String>,
        I: IntoIterator<Item = U>,
    {
        self.tags = tags.into_iter().map(Into::into).collect();
        self
    }

    /// 设置内容 MIME 类型。
    pub fn with_mime_type(mut self, mime_type: impl Into<String>) -> Self {
        self.mime_type = Some(mime_type.into());
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
    /// 资源描述更新；外层 `None` 表示不修改，内层 `None` 表示清空。
    description: Option<Option<String>>,
    /// 资源标签更新；`None` 表示不修改。
    tags: Option<Vec<String>>,
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

    /// 设置或清空资源描述。
    pub fn with_description(mut self, description: Option<String>) -> Self {
        self.description = Some(description);
        self
    }

    /// 替换全部资源标签。
    pub fn with_tags<T, I>(mut self, tags: I) -> Self
    where
        T: Into<String>,
        I: IntoIterator<Item = T>,
    {
        self.tags = Some(tags.into_iter().map(Into::into).collect());
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
    available_actions: Vec<ResourceActionDefinition>,
}

impl ResourceActions {
    fn new(available_actions: Vec<ResourceActionDefinition>) -> Self {
        Self { available_actions }
    }

    /// 返回当前资源可执行的全部动作。
    pub fn available_actions(&self) -> &[ResourceActionDefinition] {
        &self.available_actions
    }
}

/// Core 内部使用的缓冲预览结果。
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
struct ResourcePreview {
    content_type: String,
    content: Bytes,
}

#[cfg(test)]
impl ResourcePreview {
    fn new(content_type: String, content: Bytes) -> Self {
        Self {
            content_type,
            content,
        }
    }

    /// 返回内容类型。
    fn content_type(&self) -> &str {
        &self.content_type
    }

    /// 返回预览内容。
    fn content(&self) -> &Bytes {
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
/// 具体用例按职责拆分到内部命令、内容、动作和预览服务；非可信调用方只能通过
/// [`ResourceService::secured`] 获得绑定授权上下文的公开用例门面。
///
/// 对象存储和数据库之间没有分布式事务。本服务会在关键流程中做必要的顺序控制和
/// 最小补偿，但调用方仍应根据业务需要在更外层增加重试、任务补偿或审计机制。
#[derive(Clone)]
pub struct ResourceService {
    /// 资源聚合仓储端口。
    repository: Arc<dyn ResourceRepository>,
    /// 资源只读查询端口。
    query: Arc<dyn ResourceQuery>,
    /// 对象存储端口。
    blob_storage: Arc<dyn BlobStorage>,
    directory_storage: Arc<dyn DirectoryStorage>,
    storage_scanner: Arc<dyn StorageScanner>,
    /// 资源类型注册表。
    kind_registry: Arc<dyn ResourceKindRegistry>,
    /// 全局资源动作注册表。
    action_registry: Option<Arc<dyn ResourceActionRegistry>>,
    /// 插件资源动作执行器。
    action_executor: Option<Arc<dyn ResourceActionExecutor>>,
    /// 核心动作编排和插件宿主共同使用的执行限制。
    plugin_execution_policy: Arc<PluginExecutionPolicy>,
}

/// `ResourceService` 所需的 Host Port 装配。
pub struct ResourceServicePorts {
    repository: Arc<dyn ResourceRepository>,
    query: Arc<dyn ResourceQuery>,
    blob_storage: Arc<dyn BlobStorage>,
    directory_storage: Arc<dyn DirectoryStorage>,
    storage_scanner: Arc<dyn StorageScanner>,
    kind_registry: Arc<dyn ResourceKindRegistry>,
    action_registry: Option<Arc<dyn ResourceActionRegistry>>,
    action_executor: Option<Arc<dyn ResourceActionExecutor>>,
}

impl ResourceServicePorts {
    pub fn new(
        repository: Arc<dyn ResourceRepository>,
        query: Arc<dyn ResourceQuery>,
        blob_storage: Arc<dyn BlobStorage>,
        directory_storage: Arc<dyn DirectoryStorage>,
        storage_scanner: Arc<dyn StorageScanner>,
        kind_registry: Arc<dyn ResourceKindRegistry>,
    ) -> Self {
        Self {
            repository,
            query,
            blob_storage,
            directory_storage,
            storage_scanner,
            kind_registry,
            action_registry: None,
            action_executor: None,
        }
    }

    pub fn with_actions(
        mut self,
        registry: Arc<dyn ResourceActionRegistry>,
        executor: Arc<dyn ResourceActionExecutor>,
    ) -> Self {
        self.action_registry = Some(registry);
        self.action_executor = Some(executor);
        self
    }
}

impl ResourceService {
    /// 创建资源应用服务。
    ///
    /// `ports` 通常由应用启动层根据配置装配，例如 SQLite + Fs、Postgres + S3 等组合。
    /// Port 使用 trait object，使应用层可以替换具体实现而不改变 Core 用例。
    pub fn new(
        ports: ResourceServicePorts,
        plugin_execution_policy: Arc<PluginExecutionPolicy>,
    ) -> Self {
        let ResourceServicePorts {
            repository,
            query,
            blob_storage,
            directory_storage,
            storage_scanner,
            kind_registry,
            action_registry,
            action_executor,
        } = ports;
        Self {
            repository,
            query,
            blob_storage,
            directory_storage,
            storage_scanner,
            kind_registry,
            action_registry,
            action_executor,
            plugin_execution_policy,
        }
    }
    /// 返回资源命令服务。
    ///
    /// 命令服务负责资源聚合本身的生命周期变化，例如创建、更新、软删除和物理移除。
    fn commands(&self) -> ResourceCommandService<'_> {
        ResourceCommandService::new(self)
    }

    /// 返回资源内容服务。
    ///
    /// 内容服务负责对象内容的上传、流式上传和下载，并处理对象存储与资源仓储之间的补偿。
    fn content(&self) -> ResourceContentService<'_> {
        ResourceContentService::new(self)
    }

    /// 返回资源动作服务。
    ///
    /// 动作服务负责解析 kind/action 声明、执行动作和应用动作返回的写入效果。
    fn actions(&self) -> ResourceActionService<'_> {
        ResourceActionService::new(self)
    }

    /// 返回资源预览服务。
    ///
    /// 预览服务负责 read、preview、thumbnail 等面向展示的读取流程。
    fn previews(&self) -> ResourcePreviewService<'_> {
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

    pub async fn check_repository_health(&self) -> Result<(), CoreError> {
        self.repository.health_check().await
    }

    pub async fn check_blob_storage_health(&self) -> Result<(), CoreError> {
        self.blob_storage.health_check().await
    }

    /// 计算已授权资源当前可展示的动作。
    ///
    /// 本方法只描述能力，不执行插件或产生写副作用。调用者应先通过
    /// [`SecuredResourceService`] 获取资源，再将结果用于响应映射或 UI 展示。
    pub fn describe_resource_actions(
        &self,
        resource: &Resource,
    ) -> Result<ResourceActions, CoreError> {
        self.actions().describe_resource_actions(resource)
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

    fn actions_for_resource_kind(&self, kind: &ResourceKind) -> Vec<ResourceActionDefinition> {
        let lineage = self.kind_registry.lineage(kind);
        self.action_registry
            .as_ref()
            .map(|registry| registry.actions_for_kinds(&lineage))
            .unwrap_or_default()
    }

    /// 返回指定 kind 及其祖先谱系适用的动作定义。
    pub fn describe_kind_actions(&self, kind: &ResourceKind) -> Vec<ResourceActionDefinition> {
        self.actions_for_resource_kind(kind)
    }

    fn action_matches_resource(
        &self,
        action: &ResourceActionDefinition,
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
                    content
                        .map(|_| resource.storage_key())
                        .as_ref()
                        .map(StorageKey::as_str),
                )
            })
    }
}

fn resolved_content_delivery(
    action: &ResourceActionDefinition,
    size: u64,
    policy: &PluginExecutionPolicy,
) -> Option<ResourceActionContentDelivery> {
    if !action.requirements().content {
        return None;
    }
    match action.requirements().content_delivery {
        ResourceActionContentDelivery::Auto if size <= policy.max_inline_content_bytes() => {
            Some(ResourceActionContentDelivery::Inline)
        }
        ResourceActionContentDelivery::Auto => Some(ResourceActionContentDelivery::Reference),
        delivery => Some(delivery),
    }
}

fn should_load_declared_action_content(
    action: &ResourceActionDefinition,
    content: &ResourceContent,
    policy: &PluginExecutionPolicy,
) -> bool {
    matches!(
        resolved_content_delivery(action, content.size(), policy),
        Some(ResourceActionContentDelivery::Inline)
    )
}

fn decode_media_view(action: &str, view: &PluginView) -> Result<(String, Bytes), CoreError> {
    let PluginView::Media(media) = view else {
        return Err(CoreError::configuration(format!(
            "resource action `{action}` must return a media view"
        )));
    };
    if media.encoding != PluginMediaEncoding::Base64 {
        return Err(CoreError::configuration(format!(
            "resource action `{action}` returned URL media where inline media was required"
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
    description: Option<String>,
    tags: Vec<String>,
) -> crate::domain::ResourceBuilder {
    let mut builder = Resource::builder(name)
        .with_directory(directory)
        .with_status(status)
        .with_tags(tags);

    if let Some(description) = description {
        builder = builder.with_description(description);
    }

    if let Some(kind) = kind {
        builder = builder.with_kind(kind);
    }

    builder
}

fn fallback_resource_kind() -> ResourceKind {
    ResourceKind::from("core:file")
}

fn build_content(
    size: u64,
    mime_type: Option<String>,
    checksum: Checksum,
) -> Result<ResourceContent, CoreError> {
    let mut content = ResourceContent::builder(size, checksum);

    if let Some(mime_type) = mime_type {
        content = content.with_mime_type(mime_type);
    }

    Ok(content.build()?)
}

fn calculate_checksum(data: &[u8]) -> Result<Checksum, CoreError> {
    let mut state = ChecksumState::new(CONTENT_CHECKSUM_KIND);
    state.update(data);
    state.finish()
}

fn placeholder_checksum() -> Result<Checksum, CoreError> {
    calculate_checksum(&[])
}

fn stream_with_checksum_tracking(
    data: BlobByteStream,
) -> (BlobByteStream, Arc<std::sync::Mutex<ChecksumState>>) {
    let state = Arc::new(std::sync::Mutex::new(ChecksumState::new(
        CONTENT_CHECKSUM_KIND,
    )));
    let stream_state = state.clone();
    let stream = data.map(move |chunk| {
        if let Ok(chunk) = &chunk {
            stream_state
                .lock()
                .expect("checksum mutex should not be poisoned")
                .update(chunk);
        }

        chunk
    });

    (Box::pin(stream), state)
}

fn finalize_tracked_checksum(
    state: Arc<std::sync::Mutex<ChecksumState>>,
) -> Result<Checksum, CoreError> {
    state
        .lock()
        .expect("checksum mutex should not be poisoned")
        .finish()
}

enum ChecksumState {
    Sha256(Sha256),
}

impl ChecksumState {
    fn new(kind: ChecksumKind) -> Self {
        match kind {
            ChecksumKind::Sha256 => Self::Sha256(Sha256::new()),
        }
    }

    fn update(&mut self, bytes: &[u8]) {
        match self {
            Self::Sha256(state) => state.update(bytes),
        }
    }

    fn finish(&self) -> Result<Checksum, CoreError> {
        match self {
            Self::Sha256(state) => {
                Checksum::sha256(hex_digest(&state.clone().finalize())).map_err(Into::into)
            }
        }
    }
}

#[cfg(test)]
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
mod tests;
