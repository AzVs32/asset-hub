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

    /// 将对象存储的完整最终状态协调到 Resource 和目录仓储。
    pub(crate) async fn reconcile_storage(&self) -> Result<(), CoreError> {
        let prefix = StoragePrefix::root();
        let mut entries = self.service.storage_scanner.scan(&prefix);
        let mut physical_directories = std::collections::HashSet::new();
        let mut physical_keys = std::collections::HashSet::new();
        while let Some(entry) = entries.next().await {
            match entry? {
                ScannedStorageEntry::Directory(directory) => {
                    self.service.repository.ensure_directory(&directory).await?;
                    physical_directories.insert(directory);
                }
                ScannedStorageEntry::Blob(file) => {
                    physical_keys.insert(file.key.clone());
                    self.reconcile_scanned_blob(file).await?;
                }
            }
        }

        // 只有完整消费扫描流后才执行删除，避免扫描中途失败导致错误删除未见条目。
        let mut offset = 0_u64;
        let mut stored_resources = Vec::new();
        loop {
            let page = self
                .service
                .query
                .list(&ListResources::new(1_000, offset))
                .await?;
            offset += page.items.len() as u64;
            stored_resources.extend(page.items);
            if offset >= page.total {
                break;
            }
        }
        for resource in stored_resources {
            if resource.content().is_some() && !physical_keys.contains(&resource.storage_key()) {
                self.service
                    .repository
                    .remove_if_unchanged(&resource.id(), resource.updated_at())
                    .await?;
            }
        }

        self.reconcile_directories(physical_directories).await
    }

    pub(crate) async fn reconcile_storage_keys(
        &self,
        keys: &[StorageKey],
    ) -> Result<(), CoreError> {
        let mut existing = Vec::new();
        let mut missing = Vec::new();
        for key in keys {
            match self.service.storage_scanner.inspect(key).await? {
                Some(file) => existing.push(file),
                None => missing.push(key),
            }
        }
        // 先处理目标路径，再移除源路径，使拆分为 From/To 的平台重命名事件仍有机会保留 ID。
        for file in existing {
            self.reconcile_scanned_blob(file).await?;
        }
        for key in missing {
            self.remove_missing_blob_resource(key).await?;
        }
        Ok(())
    }

    pub(crate) async fn reconcile_storage_rename(
        &self,
        from: &StorageKey,
        to: &StorageKey,
    ) -> Result<(), CoreError> {
        let Some(target) = self.service.storage_scanner.inspect(to).await? else {
            return self
                .reconcile_storage_keys(&[from.clone(), to.clone()])
                .await;
        };
        let (from_directory, from_name) = resource_path_from_key(from)?;
        let Some(mut resource) = self
            .service
            .query
            .find_by_path(&from_directory, &from_name)
            .await?
        else {
            return self.reconcile_scanned_blob(target).await;
        };
        let (to_directory, to_name) = resource_path_from_key(to)?;
        if self
            .service
            .query
            .find_by_path(&to_directory, &to_name)
            .await?
            .is_some()
        {
            self.remove_missing_blob_resource(from).await?;
            return self.reconcile_scanned_blob(target).await;
        }

        let expected_updated_at = resource.updated_at();
        resource.rename(to_name)?;
        resource.move_to_directory(to_directory.clone())?;
        let existing_content = resource.content().cloned();
        if existing_content.as_ref().map(ResourceContent::size) != Some(target.size) {
            let checksum = self.calculate_stored_blob_checksum(to, target.size).await?;
            resource.attach_content(build_content(target.size, target.mime_type, checksum)?)?;
        } else if let Some(content) = existing_content
            && content.mime_type() != target.mime_type.as_deref()
        {
            resource.attach_content(build_content(
                target.size,
                target.mime_type,
                content.checksum().clone(),
            )?)?;
        }
        self.service
            .repository
            .ensure_directory(&to_directory)
            .await?;
        if !self
            .service
            .repository
            .save_if_unchanged(&resource, expected_updated_at)
            .await?
        {
            return Err(CoreError::conflict(format!(
                "resource `{}` changed while storage rename was synchronized",
                resource.id()
            )));
        }
        Ok(())
    }

    async fn reconcile_scanned_blob(&self, file: ScannedBlob) -> Result<(), CoreError> {
        let (directory, name) = resource_path_from_key(&file.key)?;
        let checksum = self
            .calculate_stored_blob_checksum(&file.key, file.size)
            .await?;
        let content = build_content(file.size, file.mime_type.clone(), checksum)?;
        if let Some(mut resource) = self.service.query.find_by_path(&directory, &name).await? {
            if resource.content() == Some(&content) {
                return Ok(());
            }
            let expected_updated_at = resource.updated_at();
            resource.attach_content(content)?;
            if !self
                .service
                .repository
                .save_if_unchanged(&resource, expected_updated_at)
                .await?
            {
                return Err(CoreError::conflict(format!(
                    "resource `{}` changed while storage content was synchronized",
                    resource.id()
                )));
            }
            return Ok(());
        }

        if let Some(mut resource) = self.find_missing_rename_candidate(&content).await? {
            let expected_updated_at = resource.updated_at();
            resource.rename(name)?;
            resource.move_to_directory(directory.clone())?;
            resource.attach_content(content)?;
            self.service.repository.ensure_directory(&directory).await?;
            if !self
                .service
                .repository
                .save_if_unchanged(&resource, expected_updated_at)
                .await?
            {
                return Err(CoreError::conflict(format!(
                    "resource `{}` changed while storage move was synchronized",
                    resource.id()
                )));
            }
            return Ok(());
        }

        let kind = self.service.resolve_content_kind(
            None,
            content.mime_type(),
            Some(file.key.as_str()),
        )?;
        let resource = build_resource(
            name,
            directory.clone(),
            Some(kind),
            ResourceStatus::default(),
            None,
            Vec::new(),
        )
        .with_content(content)
        .build()?;
        self.ensure_directory(&directory).await?;
        self.service.repository.save(&resource).await?;
        Ok(())
    }

    async fn find_missing_rename_candidate(
        &self,
        content: &ResourceContent,
    ) -> Result<Option<Resource>, CoreError> {
        let mut candidates = Vec::new();
        for resource in self.all_active_resources().await? {
            if !resource.content().is_some_and(|existing| {
                existing.size() == content.size() && existing.checksum() == content.checksum()
            }) {
                continue;
            }
            if self
                .service
                .storage_scanner
                .inspect(&resource.storage_key())
                .await?
                .is_none()
            {
                candidates.push(resource);
                if candidates.len() > 1 {
                    return Ok(None);
                }
            }
        }
        Ok(candidates.pop())
    }

    async fn all_active_resources(&self) -> Result<Vec<Resource>, CoreError> {
        let mut offset = 0_u64;
        let mut resources = Vec::new();
        loop {
            let page = self
                .service
                .query
                .list(&ListResources::new(1_000, offset))
                .await?;
            offset += page.items.len() as u64;
            resources.extend(page.items);
            if offset >= page.total {
                return Ok(resources);
            }
        }
    }

    async fn remove_missing_blob_resource(&self, key: &StorageKey) -> Result<(), CoreError> {
        let (directory, name) = resource_path_from_key(key)?;
        let Some(resource) = self.service.query.find_by_path(&directory, &name).await? else {
            return Ok(());
        };
        if resource.content().is_some() {
            self.service
                .repository
                .remove_if_unchanged(&resource.id(), resource.updated_at())
                .await?;
        }
        Ok(())
    }

    async fn reconcile_directories(
        &self,
        physical_directories: std::collections::HashSet<ResourceDirectory>,
    ) -> Result<(), CoreError> {
        let mut stored = Vec::new();
        let mut pending = vec![ResourceDirectory::root()];
        while let Some(parent) = pending.pop() {
            let children = self.service.query.list_directories(&parent).await?;
            pending.extend(children.iter().cloned());
            stored.extend(children);
        }
        stored.sort_by_key(|directory| std::cmp::Reverse(directory.path().matches('/').count()));
        for directory in stored {
            if !physical_directories.contains(&directory) {
                self.service.repository.remove_directory(&directory).await?;
            }
        }
        Ok(())
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

        let detection_storage_key = StorageKey::from_resource_path(&directory, &name)?;

        let kind = self.service.resolve_content_kind(
            kind,
            mime_type.as_deref(),
            Some(detection_storage_key.as_str()),
        )?;

        let mut resource = build_resource(
            name,
            directory.clone(),
            Some(kind),
            status,
            description,
            tags,
        )
        .build()?;
        let storage_key = resource.storage_key();
        reject_reserved_storage_key(&storage_key)?;
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
        resource.attach_content(content)?;

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

fn resource_path_from_key(key: &StorageKey) -> Result<(ResourceDirectory, String), CoreError> {
    let (directory, name) = key
        .as_str()
        .rsplit_once('/')
        .map_or(("", key.as_str()), |(directory, name)| (directory, name));
    Ok((ResourceDirectory::from_path(directory)?, name.to_owned()))
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
