//! 对象存储与资源仓储的状态协调。
//!
//! 扫描器只报告存储事实；本服务负责把最终状态投影到资源和目录仓储。只有完整扫描成功
//! 后才删除未见记录，避免一次不完整扫描造成误删。

use super::ResourceService;
use super::command::build_resource;
use super::content::{build_content, finalize_tracked_checksum, stream_with_checksum_tracking};
use crate::CoreError;
use crate::domain::{
    Checksum, Resource, ResourceContent, ResourceDirectory, ResourceStatus, StorageKey,
};
use crate::port::{ListResources, ScannedBlob, ScannedStorageEntry, StoragePrefix};
use futures_util::StreamExt;
use std::cmp::Reverse;
use std::collections::HashSet;

pub(super) struct StorageReconciliationService<'a> {
    service: &'a ResourceService,
}

impl<'a> StorageReconciliationService<'a> {
    pub(super) fn new(service: &'a ResourceService) -> Self {
        Self { service }
    }

    pub(super) async fn reconcile_storage(&self) -> Result<(), CoreError> {
        let mut entries = self.service.storage_scanner.scan(&StoragePrefix::root());
        let mut physical_directories = HashSet::new();
        let mut physical_keys = HashSet::new();
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

        for resource in self.all_active_resources().await? {
            if resource.content().is_some() && !physical_keys.contains(&resource.storage_key()) {
                self.service
                    .repository
                    .remove_if_unchanged(&resource.id(), resource.updated_at())
                    .await?;
            }
        }

        self.reconcile_directories(physical_directories).await
    }

    pub(super) async fn reconcile_storage_keys(
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

    pub(super) async fn reconcile_storage_rename(
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
        physical_directories: HashSet<ResourceDirectory>,
    ) -> Result<(), CoreError> {
        let mut stored = Vec::new();
        let mut pending = vec![ResourceDirectory::root()];
        while let Some(parent) = pending.pop() {
            let children = self.service.query.list_directories(&parent).await?;
            pending.extend(children.iter().cloned());
            stored.extend(children);
        }
        stored.sort_by_key(|directory| Reverse(directory.path().matches('/').count()));
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

    async fn ensure_directory(&self, directory: &ResourceDirectory) -> Result<(), CoreError> {
        self.service
            .directory_storage
            .ensure_directory(directory)
            .await?;
        self.service.repository.ensure_directory(directory).await
    }
}

fn resource_path_from_key(key: &StorageKey) -> Result<(ResourceDirectory, String), CoreError> {
    let (directory, name) = key
        .as_str()
        .rsplit_once('/')
        .map_or(("", key.as_str()), |(directory, name)| (directory, name));
    Ok((ResourceDirectory::from_path(directory)?, name.to_owned()))
}
