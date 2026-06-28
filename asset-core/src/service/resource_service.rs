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
use crate::port::{BlobByteStream, BlobStorage, ListResources, ResourcePage, ResourceRepository};
use bytes::Bytes;
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::sync::Arc;

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
    /// 资源类型；未设置时使用 `ResourceKind::default()`。
    kind: Option<ResourceKind>,
    /// 初始生命周期状态。
    status: ResourceStatus,
    /// 初始资源元数据。
    metadata: ResourceMetadata,
}

impl CreateResource {
    /// 创建命令，默认使用未知资源类型、活跃状态和空元数据。
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
    /// 未调用该方法时，资源类型使用 `ResourceKind::default()`。传入字符串时会在 usecase
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
    /// 资源类型；未设置时使用 `ResourceKind::default()`。
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

impl UploadResourceContent {
    /// 创建命令，默认使用未知资源类型、活跃状态和空元数据。
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
    /// 未调用该方法时，资源类型使用 `ResourceKind::default()`。
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
    /// 资源类型；未设置时使用 `ResourceKind::default()`。
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
    /// 创建流式上传命令，默认使用未知资源类型、活跃状态和空元数据。
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
}

impl ResourceService {
    /// 创建资源应用服务。
    ///
    /// `repository` 和 `blob_storage` 通常由应用启动层根据配置创建，例如 SQLite + Fs、
    /// Postgres + S3 等组合。这里使用 trait object 是为了让应用层可以替换具体实现。
    pub fn new(
        repository: Arc<dyn ResourceRepository>,
        blob_storage: Arc<dyn BlobStorage>,
    ) -> Self {
        Self {
            repository,
            blob_storage,
        }
    }

    /// 创建纯元数据资源。
    ///
    /// 该 usecase 只保存资源聚合，不写入对象存储。成功时返回已经保存的 `Resource`，
    /// 其中包含新生成的 `ResourceId`、创建时间和更新时间。
    ///
    /// 可能返回的错误包括领域校验错误和仓储保存错误。
    pub async fn create_resource(&self, command: CreateResource) -> Result<Resource, CoreError> {
        let resource =
            build_resource(command.name, command.kind, command.status, command.metadata).build()?;

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

        verify_bytes_checksums(&data, &checksums)?;

        let content = build_content(
            storage_key.clone(),
            data.len() as u64,
            mime_type,
            original_filename,
            checksums,
        )?;
        let resource = build_resource(name, kind, status, metadata)
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

        let resource_builder = build_resource(name, kind, status, metadata);
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
        self.repository.list(&query).await
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
            resource.change_kind(kind)?;
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
    use crate::port::BlobWriteResult;
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
        let repository = Arc::new(InMemoryResourceRepository::default());
        let blob_storage = Arc::new(InMemoryBlobStorage::default());
        let service = ResourceService::new(repository.clone(), blob_storage.clone());

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
                    .with_kind("asset:image")
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
