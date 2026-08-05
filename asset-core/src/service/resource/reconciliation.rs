//! 对象存储与资源仓储的状态协调。
//!
//! 扫描器只报告存储事实；本服务负责把最终状态投影到资源和目录仓储。只有完整扫描成功
//! 后才删除未见记录，避免一次不完整扫描造成误删。

use super::ResourceService;
use super::command::build_resource;
use super::content::{
    build_failed_content, build_pending_content, build_verified_content, finalize_tracked_checksum,
    stream_with_checksum_tracking,
};
use crate::CoreError;
use crate::domain::{
    Checksum, ContentVerificationStatus, DirectoryPath, Resource, ResourceContent, StorageKey,
};
use crate::port::{
    ListResources, LocatedResource, ScannedBlob, ScannedStorageEntry, StoragePrefix,
};
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
    pending_verification_keys: Vec<StorageKey>,
}

impl StorageReconciliationReport {
    pub fn directory_keys(&self) -> &[StorageKey] {
        &self.directory_keys
    }

    /// 返回第一阶段恢复后需要在后台计算校验和的对象。
    pub fn pending_verification_keys(&self) -> &[StorageKey] {
        &self.pending_verification_keys
    }
}

pub(super) struct StorageReconciliationService<'a> {
    service: &'a ResourceService,
}

impl<'a> StorageReconciliationService<'a> {
    pub(super) fn new(service: &'a ResourceService) -> Self {
        Self { service }
    }

    /// 启动时优先恢复可用的资源索引。
    ///
    /// 仓储为空或尚未产生任何已校验内容时，第一阶段仅读取对象元数据并创建 pending
    /// Resource；调用方随后并发校验 `pending_verification_keys`。已有已校验索引时继续
    /// 增量协调，但把尚未完成的校验交给后台，以保留依赖校验和识别离线重命名的语义。
    pub(super) async fn reconcile_storage_on_startup(
        &self,
    ) -> Result<StorageReconciliationReport, CoreError> {
        let resources = self.all_active_resources().await?;
        if resources.is_empty()
            || resources.iter().all(|located| {
                located.resource().content().is_some_and(|content| {
                    content.verification_status() != ContentVerificationStatus::Verified
                })
            })
        {
            self.recover_storage_metadata().await
        } else {
            self.reconcile_storage_inner(false, true).await
        }
    }

    async fn recover_storage_metadata(&self) -> Result<StorageReconciliationReport, CoreError> {
        let started = Instant::now();
        let resources = self.all_active_resources().await?;
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
                    let _storage_key_guard = self.service.storage_key_locks.lock(&file.key).await;
                    let Some(current) = self.service.storage_scanner.inspect(&file.key).await?
                    else {
                        continue;
                    };
                    self.recover_scanned_blob_metadata(&current).await?;
                    report.files += 1;
                    report.pending_verification_keys.push(current.key.clone());
                }
            }
        }

        for located in resources {
            let storage_key = located.storage_key()?;
            let resource = located.resource();
            if resource.content().is_some() && !physical_keys.contains(&storage_key) {
                let _storage_key_guard = self.service.storage_key_locks.lock(&storage_key).await;
                if self
                    .service
                    .storage_scanner
                    .inspect(&storage_key)
                    .await?
                    .is_none()
                    && self
                        .service
                        .repository
                        .remove_if_unchanged(&resource.id(), resource.revision())
                        .await?
                {
                    report.removed_resources += 1;
                }
            }
        }

        report.directory_keys = physical_directories
            .iter()
            .map(|directory| StorageKey::new(directory.path().to_owned()))
            .collect::<Result<Vec<_>, _>>()?;
        report
            .directory_keys
            .sort_by(|left, right| left.as_str().cmp(right.as_str()));
        report
            .pending_verification_keys
            .sort_by(|left, right| left.as_str().cmp(right.as_str()));
        self.reconcile_directories(physical_directories).await?;
        report.elapsed = started.elapsed();
        Ok(report)
    }

    async fn recover_scanned_blob_metadata(&self, file: &ScannedBlob) -> Result<(), CoreError> {
        let (directory, name) = resource_path_from_key(&file.key)?;
        if self
            .service
            .query
            .find_by_path(&directory, &name)
            .await?
            .is_some()
        {
            return Ok(());
        }
        let content =
            build_pending_content(file.size, file.mime_type.clone(), Some(file.modified_at))?;
        let kind = self.service.resolve_content_kind(
            None,
            content.mime_type(),
            Some(file.key.as_str()),
        )?;
        let resource = build_resource(
            name,
            self.service.directories.ensure_path(&directory).await?.id(),
            Some(kind),
            Vec::new(),
        )
        .with_content(content)
        .build()?;
        self.service.repository.save(&resource).await
    }

    pub(super) async fn reconcile_storage(
        &self,
        force_checksum: bool,
    ) -> Result<StorageReconciliationReport, CoreError> {
        self.reconcile_storage_inner(force_checksum, false).await
    }

    async fn reconcile_storage_inner(
        &self,
        force_checksum: bool,
        defer_pending_verification: bool,
    ) -> Result<StorageReconciliationReport, CoreError> {
        let started = Instant::now();
        let resources = self.all_active_resources().await?;
        let mut resources_by_key = HashMap::with_capacity(resources.len());
        for resource in &resources {
            resources_by_key.insert(resource.storage_key()?, resource.resource());
        }
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
                    let matching_content = resources_by_key
                        .get(&file.key)
                        .and_then(|resource| resource.content())
                        .filter(|content| {
                            content.size() == file.size
                                && content.modified_at() == Some(file.modified_at)
                        });
                    if !force_checksum
                        && defer_pending_verification
                        && matching_content.is_some_and(|content| {
                            content.verification_status() != ContentVerificationStatus::Verified
                        })
                    {
                        report.pending_verification_keys.push(file.key.clone());
                        continue;
                    }
                    let unchanged = !force_checksum
                        && matching_content.is_some_and(|content| {
                            content.verification_status() == ContentVerificationStatus::Verified
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

        for located in resources {
            let storage_key = located.storage_key()?;
            let resource = located.resource();
            if resource.content().is_some() && !physical_keys.contains(&storage_key) {
                let _storage_key_guard = self.service.storage_key_locks.lock(&storage_key).await;
                if self
                    .service
                    .storage_scanner
                    .inspect(&storage_key)
                    .await?
                    .is_none()
                    && self
                        .service
                        .repository
                        .remove_if_unchanged(&resource.id(), resource.revision())
                        .await?
                {
                    report.removed_resources += 1;
                }
            }
        }
        report.directory_keys = physical_directories
            .iter()
            .map(|directory| StorageKey::new(directory.path().to_owned()))
            .collect::<Result<Vec<_>, _>>()?;
        report
            .directory_keys
            .sort_by(|left, right| left.as_str().cmp(right.as_str()));
        report
            .pending_verification_keys
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
            if self.service.storage_scanner.inspect(key).await?.is_some() {
                existing.push(key);
            } else {
                missing.push(key);
            }
        }

        // 先处理目标路径，再移除源路径，使拆分为 From/To 的平台重命名事件仍有机会保留 ID。
        existing.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        existing.dedup();
        missing.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        missing.dedup();
        for key in existing.into_iter().chain(missing) {
            self.reconcile_storage_key(key).await?;
        }
        Ok(())
    }

    pub(super) async fn reconcile_storage_rename(
        &self,
        from: &StorageKey,
        to: &StorageKey,
    ) -> Result<(), CoreError> {
        let _storage_key_guards = self
            .service
            .storage_key_locks
            .lock_many(&[from.clone(), to.clone()])
            .await;
        let Some(target) = self.service.storage_scanner.inspect(to).await? else {
            self.reconcile_storage_key_locked(from).await?;
            self.reconcile_storage_key_locked(to).await?;
            return Ok(());
        };
        let (from_directory, from_name) = resource_path_from_key(from)?;
        let Some(located) = self
            .service
            .query
            .find_by_path(&from_directory, &from_name)
            .await?
        else {
            self.reconcile_scanned_blob(&target).await?;
            return Ok(());
        };
        let mut resource = located.into_resource();
        let (to_directory, to_name) = resource_path_from_key(to)?;
        if self
            .service
            .query
            .find_by_path(&to_directory, &to_name)
            .await?
            .is_some()
        {
            self.remove_missing_blob_resource_locked(from).await?;
            self.reconcile_scanned_blob(&target).await?;
            return Ok(());
        }

        let expected_revision = resource.revision();
        resource.rename(to_name)?;
        let to_directory = self.service.directories.ensure_path(&to_directory).await?;
        resource.move_to_directory(to_directory.id())?;
        let checksum = self.calculate_stored_blob_checksum(to, target.size).await?;
        let content = build_verified_content(
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
            .save_if_unchanged(&resource, expected_revision)
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
        let _storage_key_guard = self.service.storage_key_locks.lock(&file.key).await;
        let Some(current) = self.service.storage_scanner.inspect(&file.key).await? else {
            self.remove_missing_blob_resource_locked(&file.key).await?;
            return Ok(Duration::ZERO);
        };
        self.reconcile_scanned_blob(&current).await
    }

    async fn reconcile_storage_key(&self, key: &StorageKey) -> Result<(), CoreError> {
        let _storage_key_guard = self.service.storage_key_locks.lock(key).await;
        self.reconcile_storage_key_locked(key).await
    }

    async fn reconcile_storage_key_locked(&self, key: &StorageKey) -> Result<(), CoreError> {
        match self.service.storage_scanner.inspect(key).await? {
            Some(file) => {
                self.reconcile_scanned_blob(&file).await?;
            }
            None => self.remove_missing_blob_resource_locked(key).await?,
        }
        Ok(())
    }

    async fn reconcile_scanned_blob(&self, file: &ScannedBlob) -> Result<Duration, CoreError> {
        let (directory, name) = resource_path_from_key(&file.key)?;
        let hash_started = Instant::now();
        let checksum = match self
            .calculate_stored_blob_checksum(&file.key, file.size)
            .await
        {
            Ok(checksum) => checksum,
            Err(error) => {
                self.mark_verification_failed(file, &error).await?;
                return Err(error);
            }
        };
        let hash_elapsed = hash_started.elapsed();
        let content = build_verified_content(
            file.size,
            file.mime_type.clone(),
            checksum,
            Some(file.modified_at),
        )?;
        if let Some(located) = self.service.query.find_by_path(&directory, &name).await? {
            let mut resource = located.into_resource();
            if resource.content() == Some(&content) {
                return Ok(hash_elapsed);
            }
            let expected_revision = resource.revision();
            resource.attach_content(content)?;
            if !self
                .service
                .repository
                .save_if_unchanged(&resource, expected_revision)
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
            let expected_revision = resource.revision();
            resource.rename(name)?;
            let directory = self.service.directories.ensure_path(&directory).await?;
            resource.move_to_directory(directory.id())?;
            resource.attach_content(content)?;
            if !self
                .service
                .repository
                .save_if_unchanged(&resource, expected_revision)
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
            self.service.directories.ensure_path(&directory).await?.id(),
            Some(kind),
            Vec::new(),
        )
        .with_content(content)
        .build()?;
        self.service.repository.save(&resource).await?;
        Ok(hash_elapsed)
    }

    async fn mark_verification_failed(
        &self,
        file: &ScannedBlob,
        error: &CoreError,
    ) -> Result<(), CoreError> {
        let Some(current) = self.service.storage_scanner.inspect(&file.key).await? else {
            return Ok(());
        };
        if current.size != file.size || current.modified_at != file.modified_at {
            return Ok(());
        }

        let (directory, name) = resource_path_from_key(&file.key)?;
        let content = build_failed_content(
            file.size,
            file.mime_type.clone(),
            error.to_string(),
            Some(file.modified_at),
        )?;
        if let Some(located) = self.service.query.find_by_path(&directory, &name).await? {
            let mut resource = located.into_resource();
            let expected_revision = resource.revision();
            resource.attach_content(content)?;
            if !self
                .service
                .repository
                .save_if_unchanged(&resource, expected_revision)
                .await?
            {
                return Err(CoreError::conflict(format!(
                    "resource `{}` changed while checksum failure was recorded",
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
            self.service.directories.ensure_path(&directory).await?.id(),
            Some(kind),
            Vec::new(),
        )
        .with_content(content)
        .build()?;
        self.service.repository.save(&resource).await
    }

    async fn find_missing_rename_candidate(
        &self,
        content: &ResourceContent,
    ) -> Result<Option<Resource>, CoreError> {
        let mut candidates = Vec::new();
        let Some(checksum) = content.checksum() else {
            return Ok(None);
        };
        for located in self.all_active_resources().await? {
            let resource = located.resource();
            if !resource.content().is_some_and(|existing| {
                existing.size() == content.size() && existing.checksum() == Some(checksum)
            }) {
                continue;
            }
            let storage_key = located.storage_key()?;
            if self
                .service
                .storage_scanner
                .inspect(&storage_key)
                .await?
                .is_none()
            {
                candidates.push(located.into_resource());
                if candidates.len() > 1 {
                    return Ok(None);
                }
            }
        }
        Ok(candidates.pop())
    }

    async fn all_active_resources(&self) -> Result<Vec<LocatedResource>, CoreError> {
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

    async fn remove_missing_blob_resource_locked(&self, key: &StorageKey) -> Result<(), CoreError> {
        let (directory, name) = resource_path_from_key(key)?;
        if let Some(located) = self.service.query.find_by_path(&directory, &name).await?
            && located.resource().content().is_some()
        {
            let resource = located.resource();
            self.service
                .repository
                .remove_if_unchanged(&resource.id(), resource.revision())
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
