//! 资源内容服务。
//!
//! 本模块负责对象内容的流式写入、导入和读取，包括校验和校验以及仓储保存失败后的对象清理补偿。

use super::*;
use std::collections::HashSet;

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

    /// 扫描对象存储并为尚未登记的对象创建资源记录。
    pub async fn scan_storage(&self, command: ScanStorage) -> Result<ScanStorageResult, CoreError> {
        const MAX_SCAN_ENTRIES: usize = 100_000;

        let directory = command.directory;
        let files = self
            .service
            .storage_scanner
            .scan(&directory, command.include_sha256, MAX_SCAN_ENTRIES)
            .await?;
        let scanned = files.len() as u64;
        let scanned_keys = files
            .iter()
            .map(|file| file.key.as_str().to_owned())
            .collect::<HashSet<_>>();
        let mut resources = Vec::new();
        let mut errors = Vec::new();
        let mut skipped = 0_u64;

        for file in files {
            let key = file.key.as_str().to_owned();
            let (file_directory, name) = key
                .rsplit_once('/')
                .map(|(directory, name)| (directory.to_owned(), name.to_owned()))
                .unwrap_or_else(|| (String::new(), key.clone()));
            let mut import = ImportResourceContent::new(name.clone(), file.key, file.size)
                .with_directory(ResourceDirectory::from_path(file_directory)?)
                .with_original_filename(name);
            if let Some(mime_type) = file.mime_type {
                import = import.with_mime_type(mime_type);
            }
            if let Some(sha256) = file.sha256 {
                import = import.with_checksum(Checksum::sha256(sha256)?);
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

        let stored_resources = self
            .service
            .repository
            .list(&ListResources::new(u32::MAX, 0).with_include_deleted(true))
            .await?;
        let prefix = (!directory.is_root()).then(|| format!("{directory}/"));
        for resource in stored_resources.items {
            let Some(content) = resource.content() else {
                continue;
            };
            let key = content.key().as_str();
            let in_scope = prefix.as_ref().is_none_or(|prefix| key.starts_with(prefix));
            if in_scope && !scanned_keys.contains(key) {
                errors.push(ScanStorageError {
                    key: key.to_owned(),
                    error: "resource references a missing blob".to_owned(),
                });
            }
        }

        Ok(ScanStorageResult {
            scanned_directory: directory,
            scanned,
            skipped,
            errors,
            resources,
        })
    }

    /// 导入已存在对象内容并创建资源。
    ///
    /// 该 usecase 不写入对象存储，只保存指向现有对象的内容引用。若相同 storage key 已有
    /// 资源记录，则返回 `Ok(None)`，用于支持扫描任务幂等执行。
    pub async fn import_resource_content(
        &self,
        command: ImportResourceContent,
    ) -> Result<Option<Resource>, CoreError> {
        let ImportResourceContent {
            name,
            kind,
            status,
            directory,
            metadata,
            storage_key,
            payload: size,
            mime_type,
            original_filename,
            checksums,
        } = command;

        if self
            .service
            .repository
            .find_by_content_key(&storage_key)
            .await?
            .is_some()
        {
            return Ok(None);
        }

        let kind = self.service.resolve_content_kind(
            kind,
            mime_type.as_deref(),
            original_filename
                .as_deref()
                .or_else(|| Some(storage_key.as_str())),
        )?;
        let content = build_content(storage_key, size, mime_type, original_filename, checksums)?;
        let resource = build_resource(name, directory, Some(kind), status, metadata)
            .with_content(content)
            .build()?;

        self.service.repository.save(&resource).await?;

        Ok(Some(resource))
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
            directory,
            metadata,
            storage_key,
            payload: data,
            mime_type,
            original_filename,
            mut checksums,
        } = command;
        let kind = self.service.resolve_content_kind(
            kind,
            mime_type.as_deref(),
            original_filename
                .as_deref()
                .or_else(|| Some(storage_key.as_str())),
        )?;

        let resource_builder = build_resource(name, directory, Some(kind), status, metadata);
        resource_builder.clone().build()?;
        build_content(
            storage_key.clone(),
            0,
            mime_type.clone(),
            original_filename.clone(),
            checksums.clone(),
        )?;

        let (data, sha256_state) = stream_with_checksum_tracking(data);
        let write_result = self
            .service
            .blob_storage
            .put_stream_if_absent(&storage_key, data)
            .await?;
        let actual_sha256 = match finalize_tracked_checksum(sha256_state, &checksums) {
            Ok(checksum) => checksum,
            Err(error) => {
                let _ = self.service.blob_storage.delete(&storage_key).await;
                return Err(error);
            }
        };
        if sha256_checksum(&checksums).is_none() {
            checksums.push(actual_sha256);
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
