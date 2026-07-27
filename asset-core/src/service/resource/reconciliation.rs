//! 对象存储与资源仓储的状态协调。
//!
//! 扫描器只报告存储事实；本服务负责把最终状态投影到资源和目录仓储。只有完整扫描成功
//! 后才删除未见记录，避免一次不完整扫描造成误删。

use super::ResourceService;
use super::command::build_resource;
use super::content::{build_content, finalize_tracked_checksum, stream_with_checksum_tracking};
use crate::CoreError;
use crate::domain::{
    Checksum, DirectoryPath, Resource, ResourceContent, ResourceStatus, StorageKey,
};
use crate::port::{ListResources, ScannedBlob, ScannedStorageEntry, StoragePrefix};
use futures_util::StreamExt;
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StorageReconciliationReport {
    pub files: u64,
    pub hashed_files: u64,
    pub unchanged_files: u64,
    pub directories: u64,
    pub removed_resources: u64,
    pub elapsed: Duration,
    pub hash_elapsed: Duration,
    directory_keys: Vec<StorageKey>,
}

impl StorageReconciliationReport {
    pub fn directory_keys(&self) -> &[StorageKey] {
        &self.directory_keys
    }
}

pub(super) struct StorageReconciliationService<'a> {
    service: &'a ResourceService,
}

impl<'a> StorageReconciliationService<'a> {
    pub(super) fn new(service: &'a ResourceService) -> Self {
        Self { service }
    }

    pub(super) async fn reconcile_storage(
        &self,
        force_checksum: bool,
    ) -> Result<StorageReconciliationReport, CoreError> {
        let started = Instant::now();
        let resources = self.all_active_resources().await?;
        let resources_by_key = resources
            .iter()
            .map(|resource| (resource.storage_key(), resource))
            .collect::<HashMap<_, _>>();
        let mut entries = self.service.storage_scanner.scan(&StoragePrefix::root());
        let mut physical_directories = HashSet::new();
        let mut physical_keys = HashSet::new();
        let mut report = StorageReconciliationReport::default();
        while let Some(entry) = entries.next().await {
            match entry? {
                ScannedStorageEntry::Directory(directory) => {
                    self.service.directories.ensure_path(&directory).await?;
                    physical_directories.insert(directory);
                    report.directories += 1;
                }
                ScannedStorageEntry::Blob(file) => {
                    physical_keys.insert(file.key.clone());
                    report.files += 1;
                    let unchanged = !force_checksum
                        && resources_by_key.get(&file.key).is_some_and(|resource| {
                            resource.content().is_some_and(|content| {
                                content.size() == file.size
                                    && content.modified_at() == Some(file.modified_at)
                            })
                        });
                    if unchanged {
                        report.unchanged_files += 1;
                        continue;
                    }
                    report.hash_elapsed += self.reconcile_changed_blob(&file).await?;
                    report.hashed_files += 1;
                }
            }
        }

        for resource in resources {
            if resource.content().is_some()
                && !physical_keys.contains(&resource.storage_key())
                && self
                    .service
                    .repository
                    .remove_if_unchanged(&resource.id(), resource.updated_at())
                    .await?
            {
                report.removed_resources += 1;
            }
        }
        report.directory_keys = physical_directories
            .iter()
            .map(|directory| StorageKey::new(directory.path().to_owned()))
            .collect::<Result<Vec<_>, _>>()?;
        report
            .directory_keys
            .sort_by(|left, right| left.as_str().cmp(right.as_str()));
        self.reconcile_directories(physical_directories).await?;
        report.elapsed = started.elapsed();
        Ok(report)
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
            self.reconcile_changed_blob(&file).await?;
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
            self.reconcile_changed_blob(&target).await?;
            return Ok(());
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
            self.reconcile_changed_blob(&target).await?;
            return Ok(());
        }

        let expected_updated_at = resource.updated_at();
        resource.rename(to_name)?;
        let to_directory = self.service.directories.ensure_path(&to_directory).await?;
        resource.move_to_directory(to_directory)?;
        let checksum = self.calculate_stored_blob_checksum(to, target.size).await?;
        let content = build_content(
            target.size,
            target.mime_type.clone(),
            checksum,
            Some(target.modified_at),
        )?;
        if resource.content() != Some(&content) {
            resource.attach_content(content)?;
        }
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

    async fn reconcile_changed_blob(&self, file: &ScannedBlob) -> Result<Duration, CoreError> {
        self.reconcile_scanned_blob(file).await
    }

    async fn reconcile_scanned_blob(&self, file: &ScannedBlob) -> Result<Duration, CoreError> {
        let (directory, name) = resource_path_from_key(&file.key)?;
        let hash_started = Instant::now();
        let checksum = self
            .calculate_stored_blob_checksum(&file.key, file.size)
            .await?;
        let hash_elapsed = hash_started.elapsed();
        let content = build_content(
            file.size,
            file.mime_type.clone(),
            checksum,
            Some(file.modified_at),
        )?;
        if let Some(mut resource) = self.service.query.find_by_path(&directory, &name).await? {
            if resource.content() == Some(&content) {
                return Ok(hash_elapsed);
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
            return Ok(hash_elapsed);
        }

        if let Some(mut resource) = self.find_missing_rename_candidate(&content).await? {
            let expected_updated_at = resource.updated_at();
            resource.rename(name)?;
            let directory = self.service.directories.ensure_path(&directory).await?;
            resource.move_to_directory(directory)?;
            resource.attach_content(content)?;
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
            return Ok(hash_elapsed);
        }

        let kind = self.service.resolve_content_kind(
            None,
            content.mime_type(),
            Some(file.key.as_str()),
        )?;
        let resource = build_resource(
            name,
            self.service.directories.ensure_path(&directory).await?,
            Some(kind),
            ResourceStatus::default(),
            Vec::new(),
        )
        .with_content(content)
        .build()?;
        self.service.repository.save(&resource).await?;
        Ok(hash_elapsed)
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
        if let Some(resource) = self.service.query.find_by_path(&directory, &name).await?
            && resource.content().is_some()
        {
            self.service
                .repository
                .remove_if_unchanged(&resource.id(), resource.updated_at())
                .await?;
        }
        Ok(())
    }

    async fn reconcile_directories(
        &self,
        physical_directories: HashSet<DirectoryPath>,
    ) -> Result<(), CoreError> {
        let mut stored = Vec::new();
        let mut pending = vec![self.service.directories.root().await?];
        while let Some(parent) = pending.pop() {
            let children = self.service.directories.list_children(&parent).await?;
            pending.extend(children.iter().cloned());
            stored.extend(children);
        }
        stored.sort_by_key(|directory| Reverse(directory.path().path().matches('/').count()));
        for directory in stored {
            if !physical_directories.contains(directory.path()) {
                self.service.directories.remove_if_empty(&directory).await?;
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
}

fn resource_path_from_key(key: &StorageKey) -> Result<(DirectoryPath, String), CoreError> {
    let (directory, name) = key
        .as_str()
        .rsplit_once('/')
        .map_or(("", key.as_str()), |(directory, name)| (directory, name));
    Ok((DirectoryPath::from_path(directory)?, name.to_owned()))
}
