//! 资源内容服务。
//!
//! 本模块负责对象内容的写入和读取，包括普通上传、流式上传、校验和校验，以及仓储保存失败后的对象清理补偿。

use super::*;

/// 资源内容服务。
///
/// 内容服务只处理资源内容引用与对象存储之间的编排，资源基础字段变更由命令服务负责。
pub struct ResourceContentService<'a> {
    service: &'a ResourceService,
}

impl<'a> ResourceContentService<'a> {
    /// 创建资源内容服务。
    pub(super) fn new(service: &'a ResourceService) -> Self {
        Self { service }
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
        let kind = self.service.resolve_content_kind(
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

        self.service
            .blob_storage
            .put_if_absent(&storage_key, data)
            .await?;

        if let Err(error) = self.service.repository.save(&resource).await {
            let _ = self.service.blob_storage.delete(&storage_key).await;
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
        let kind = self.service.resolve_content_kind(
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
            .service
            .blob_storage
            .put_stream_if_absent(&storage_key, data)
            .await?;
        if let Err(error) = verify_tracked_checksums(sha256_state, &checksums) {
            let _ = self.service.blob_storage.delete(&storage_key).await;
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

        if let Err(error) = self.service.repository.save(&resource).await {
            let _ = self.service.blob_storage.delete(&storage_key).await;
            return Err(error);
        }

        Ok(resource)
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
        let Some(resource) = self.service.repository.find_by_id(id).await? else {
            return Ok(None);
        };

        if resource.is_deleted() {
            return Ok(None);
        }

        let Some(content) = resource.content() else {
            return Ok(None);
        };

        self.service.blob_storage.get(content.key()).await
    }
}
