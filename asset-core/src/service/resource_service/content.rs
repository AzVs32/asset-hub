//! 资源内容服务。
//!
//! 本模块负责对象内容的流式写入、导入和读取，包括校验和校验以及仓储保存失败后的对象清理补偿。

use super::*;

/// 资源内容服务。
///
/// 内容服务只处理资源内容引用与对象存储之间的编排，资源基础字段变更由命令服务负责。
pub(super) struct ResourceContentService<'a> {
    service: &'a ResourceService,
}

impl<'a> ResourceContentService<'a> {
    /// 创建资源内容服务。
    pub(super) fn new(service: &'a ResourceService) -> Self {
        Self { service }
    }

    /// 扫描对象存储并为尚未登记的对象创建资源记录。
    pub(crate) async fn scan_storage(
        &self,
        command: ScanStorage,
    ) -> Result<ScanStorageResult, CoreError> {
        const MAX_SCAN_ENTRIES: usize = 100_000;

        let prefix = command.prefix;
        let directories = self
            .service
            .storage_scanner
            .scan_directories(&prefix, MAX_SCAN_ENTRIES)
            .await?;
        for directory in directories {
            self.service.repository.ensure_directory(&directory).await?;
        }

        let files = self
            .service
            .storage_scanner
            .scan(&prefix, MAX_SCAN_ENTRIES)
            .await?;
        let scanned = files.len() as u64;
        let mut resources = Vec::new();
        let mut errors = Vec::new();
        let mut skipped = 0_u64;

        for file in files {
            let key = file.key.as_str().to_owned();
            let (file_directory, name) = key
                .rsplit_once('/')
                .map(|(directory, name)| (directory.to_owned(), name.to_owned()))
                .unwrap_or_else(|| (String::new(), key.clone()));
            let mut import = ImportResourceContent::new(name, file.size)
                .with_directory(ResourceDirectory::from_path(file_directory)?);
            if let Some(mime_type) = file.mime_type {
                import = import.with_mime_type(mime_type);
            }

            match self.import_resource_content(import).await {
                Ok(Some(resource)) => resources.push(resource),
                Ok(None) => skipped += 1,
                Err(error) => {
                    skipped += 1;
                    errors.push(ScanStorageError {
                        key,
                        error: error.to_string(),
                    });
                }
            }
        }

        Ok(ScanStorageResult {
            scanned_prefix: prefix,
            scanned,
            skipped,
            errors,
            resources,
        })
    }

    /// 导入已存在对象内容并创建资源。
    ///
    /// 该 usecase 不写入对象存储，只保存指向现有对象的内容引用。若相同资源路径已有
    /// 资源记录，则返回 `Ok(None)`，用于支持扫描任务幂等执行。
    pub(crate) async fn import_resource_content(
        &self,
        command: ImportResourceContent,
    ) -> Result<Option<Resource>, CoreError> {
        let ImportResourceContent {
            name,
            kind,
            status,
            directory,
            description,
            tags,
            payload: size,
            mime_type,
        } = command;

        let storage_key = StorageKey::from_resource_path(&directory, &name)?;
        reject_reserved_storage_key(&storage_key)?;

        if self
            .service
            .query
            .find_by_path(&directory, &name)
            .await?
            .is_some()
        {
            return Ok(None);
        }

        let kind = self.service.resolve_content_kind(
            kind,
            mime_type.as_deref(),
            Some(storage_key.as_str()),
        )?;
        let checksum = self
            .calculate_stored_blob_checksum(&storage_key, size)
            .await?;
        let content = build_content(size, mime_type, checksum)?;
        let resource = build_resource(name, directory, Some(kind), status, description, tags)
            .with_content(content)
            .build()?;

        self.ensure_directory(resource.directory()).await?;
        self.service.repository.save(&resource).await?;

        Ok(Some(resource))
    }

    async fn calculate_stored_blob_checksum(
        &self,
        key: &StorageKey,
        expected_size: u64,
    ) -> Result<Checksum, CoreError> {
        let stream = self
            .service
            .blob_storage
            .get_stream(key)
            .await?
            .ok_or_else(|| CoreError::conflict(format!("blob `{key}` no longer exists")))?;
        let (mut stream, state) = stream_with_checksum_tracking(stream);
        let mut actual_size = 0_u64;

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            actual_size = actual_size
                .checked_add(chunk.len() as u64)
                .ok_or_else(|| CoreError::configuration("blob size exceeds u64"))?;
        }

        if actual_size != expected_size {
            return Err(CoreError::conflict(format!(
                "blob `{key}` changed while its checksum was being calculated"
            )));
        }

        finalize_tracked_checksum(state)
    }

    /// 流式上传对象内容并创建资源。
    ///
    /// 该 usecase 面向大文件上传。内容会以 chunk 流的形式写入 `BlobStorage`，不会在
    /// service 层聚合成完整 `Bytes`。写入完成后，service 使用存储端口返回的实际字节数
    /// 构建 `ResourceContent` 并保存资源聚合。
    ///
    /// 如果资源保存失败，本方法会尝试删除刚写入的对象内容。该补偿删除是 best-effort，
    /// 不会覆盖原始仓储错误。
    pub(crate) async fn upload_resource_content_stream(
        &self,
        command: UploadResourceContentStream,
    ) -> Result<Resource, CoreError> {
        let UploadResourceContentStream {
            name,
            kind,
            status,
            directory,
            description,
            tags,
            payload: data,
            mime_type,
        } = command;

        let storage_key = StorageKey::from_resource_path(&directory, &name)?;
        reject_reserved_storage_key(&storage_key)?;

        let kind = self.service.resolve_content_kind(
            kind,
            mime_type.as_deref(),
            Some(storage_key.as_str()),
        )?;

        let resource_builder = build_resource(
            name,
            directory.clone(),
            Some(kind),
            status,
            description,
            tags,
        );
        resource_builder.clone().build()?;
        build_content(0, mime_type.clone(), placeholder_checksum()?)?;

        self.ensure_directory(&directory).await?;
        let (data, checksum_state) = stream_with_checksum_tracking(data);
        let write_result = self
            .service
            .blob_storage
            .put_stream_if_absent(&storage_key, data)
            .await?;
        let checksum = match finalize_tracked_checksum(checksum_state) {
            Ok(checksum) => checksum,
            Err(error) => {
                let _ = self.service.blob_storage.delete(&storage_key).await;
                return Err(error);
            }
        };
        let content = build_content(write_result.bytes_written(), mime_type, checksum)?;
        let resource = resource_builder.with_content(content).build()?;

        if let Err(error) = self.service.repository.save(&resource).await {
            let _ = self.service.blob_storage.delete(&storage_key).await;
            return Err(error);
        }

        Ok(resource)
    }

    /// 确保内容写入前，其用户可见父目录同时存在于存储端和目录仓储。
    async fn ensure_directory(&self, directory: &ResourceDirectory) -> Result<(), CoreError> {
        self.service
            .directory_storage
            .ensure_directory(directory)
            .await?;
        self.service.repository.ensure_directory(directory).await
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
    #[cfg(test)]
    pub(crate) async fn get_resource_content(
        &self,
        id: &ResourceId,
    ) -> Result<Option<Bytes>, CoreError> {
        let Some(resource) = self.service.repository.find_by_id(id).await? else {
            return Ok(None);
        };

        self.get_resource_content_snapshot(&resource).await
    }

    pub(crate) async fn get_resource_content_snapshot(
        &self,
        resource: &Resource,
    ) -> Result<Option<Bytes>, CoreError> {
        if resource.is_deleted() {
            return Ok(None);
        }

        if resource.content().is_none() {
            return Ok(None);
        }

        self.service.blob_storage.get(&resource.storage_key()).await
    }

    pub(crate) async fn get_resource_content_stream_snapshot(
        &self,
        resource: &Resource,
        range: Option<(u64, u64)>,
    ) -> Result<Option<ResourceContentStream>, CoreError> {
        if resource.is_deleted() {
            return Ok(None);
        }

        let Some(content) = resource.content() else {
            return Ok(None);
        };

        let stream = if let Some((start, end)) = range {
            self.service
                .blob_storage
                .get_range_stream(&resource.storage_key(), start, end)
                .await?
        } else {
            self.service
                .blob_storage
                .get_stream(&resource.storage_key())
                .await?
        };

        Ok(stream.map(|content_stream| {
            ResourceContentStream::new(
                content_type_for_media(content),
                content.size(),
                content_stream,
            )
        }))
    }
}

fn reject_reserved_storage_key(key: &StorageKey) -> Result<(), CoreError> {
    if key.as_str() == crate::port::RESERVED_BLOB_STORAGE_PREFIX
        || key
            .as_str()
            .starts_with(&format!("{}/", crate::port::RESERVED_BLOB_STORAGE_PREFIX))
    {
        return Err(CoreError::configuration(format!(
            "storage key `{key}` uses reserved Asset Hub namespace"
        )));
    }

    Ok(())
}
