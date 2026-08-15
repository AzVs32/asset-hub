//! 资源内容的上传与读取。
//!
//! 本模块只处理资源内容引用与 Blob 之间的编排；后台扫描协调位于 `reconciliation`。

use super::{ReplaceResourceContent, ResourceContentStream, ResourceService};
use crate::CoreError;
use crate::domain::{
    Checksum, ChecksumKind, Resource, ResourceContent, ResourceContentReplacement,
    ResourceContentReplacementId, StorageKey,
};
use crate::port::{BlobByteStream, LocatedResource, RESERVED_BLOB_STORAGE_PREFIX, StagedBlob};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};

pub(super) struct ResourceContentService<'a> {
    service: &'a ResourceService,
}

impl<'a> ResourceContentService<'a> {
    pub(super) fn new(service: &'a ResourceService) -> Self {
        Self { service }
    }

    pub(crate) async fn get_resource_content_snapshot(
        &self,
        located: &LocatedResource,
    ) -> Result<Option<Bytes>, CoreError> {
        let resource = located.resource();
        if resource.is_deleted() || resource.content().is_none() {
            return Ok(None);
        }
        let storage_key = located.storage_key()?;
        self.service.blob_storage.get(&storage_key).await
    }

    pub(crate) async fn get_resource_content_stream_snapshot(
        &self,
        located: &LocatedResource,
        range: Option<(u64, u64)>,
    ) -> Result<Option<ResourceContentStream>, CoreError> {
        let resource = located.resource();
        if resource.is_deleted() {
            return Ok(None);
        }
        let Some(content) = resource.content() else {
            return Ok(None);
        };

        let storage_key = located.storage_key()?;
        let stream = if let Some((start, end)) = range {
            self.service
                .blob_storage
                .get_range_stream(&storage_key, start, end)
                .await?
        } else {
            self.service.blob_storage.get_stream(&storage_key).await?
        };

        Ok(stream.map(|content_stream| {
            ResourceContentStream::new(
                content_type_for_media(content),
                content.size(),
                content_stream,
            )
        }))
    }

    pub(crate) async fn replace_content_snapshot(
        &self,
        located: LocatedResource,
        command: ReplaceResourceContent,
        data: BlobByteStream,
    ) -> Result<Resource, CoreError> {
        let (mut resource, directory) = located.into_parts();
        if resource.revision() != command.expected_revision {
            return Err(stale_replacement(&resource));
        }
        let current_content = resource.content().cloned().ok_or_else(|| {
            CoreError::invalid_operation("resource content replacement requires existing content")
        })?;
        let has_text_edit = self
            .service
            .available_actions_for_resource(&resource)
            .iter()
            .any(|action| {
                action
                    .provides()
                    .is_some_and(|capability| capability.as_str() == "text_edit")
            });
        if !has_text_edit {
            return Err(CoreError::invalid_operation(format!(
                "resource `{}` is not available for text editing",
                resource.id()
            )));
        }
        let max_text_bytes = self.service.resource_content_edit_policy.max_text_bytes();
        if command.expected_size > max_text_bytes {
            return Err(CoreError::limit_exceeded(
                "resource text content",
                max_text_bytes,
                command.expected_size,
            ));
        }
        let target_key = StorageKey::from_resource_path(directory.path(), resource.name())?;
        let replacement_id = ResourceContentReplacementId::new();
        let backup_key = replacement_backup_key(replacement_id)?;
        let staging_key = replacement_staging_key(replacement_id)?;
        let staging = self
            .service
            .blob_storage
            .create_staged(&staging_key)
            .await?;
        let (tracked, checksum_state) = stream_with_checksum_tracking(limit_replacement_stream(
            data,
            command.expected_size,
            max_text_bytes,
        ));
        let staged = match self
            .service
            .blob_storage
            .append_staged(&staging_key, 0, tracked)
            .await
        {
            Ok(staged) => staged,
            Err(error) => {
                let _ = self.service.blob_storage.discard_staged(&staging).await;
                return Err(error);
            }
        };
        let actual_checksum = match finalize_tracked_checksum(checksum_state) {
            Ok(checksum) => checksum,
            Err(error) => {
                let _ = self.service.blob_storage.discard_staged(&staged).await;
                return Err(error);
            }
        };
        if staged.bytes_written() != command.expected_size {
            let _ = self.service.blob_storage.discard_staged(&staged).await;
            return Err(CoreError::conflict(format!(
                "content size mismatch: expected {}, received {}",
                command.expected_size,
                staged.bytes_written()
            )));
        }
        if actual_checksum != command.expected_checksum {
            let _ = self.service.blob_storage.discard_staged(&staged).await;
            return Err(CoreError::conflict(format!(
                "content checksum mismatch: expected {}, actual {}",
                command.expected_checksum.value(),
                actual_checksum.value()
            )));
        }

        let content = match build_verified_content(
            staged.bytes_written(),
            command
                .mime_type
                .or_else(|| current_content.mime_type().map(str::to_string)),
            actual_checksum,
            None,
        ) {
            Ok(content) => content,
            Err(error) => {
                let _ = self.service.blob_storage.discard_staged(&staged).await;
                return Err(error);
            }
        };
        self.commit_staged_replacement(
            &mut resource,
            target_key,
            staged,
            replacement_id,
            backup_key,
            content,
        )
        .await?;
        Ok(resource)
    }

    pub(super) async fn replace_content_bytes_snapshot(
        &self,
        resource: &mut Resource,
        target_key: &StorageKey,
        content: ResourceContent,
        data: Bytes,
    ) -> Result<(), CoreError> {
        let replacement_id = ResourceContentReplacementId::new();
        let staging_key = replacement_staging_key(replacement_id)?;
        let backup_key = replacement_backup_key(replacement_id)?;
        let staging = self
            .service
            .blob_storage
            .create_staged(&staging_key)
            .await?;
        let expected_size = data.len() as u64;
        let staged = match self
            .service
            .blob_storage
            .append_staged(
                &staging_key,
                0,
                Box::pin(futures_util::stream::once(async move { Ok(data) })),
            )
            .await
        {
            Ok(staged) if staged.bytes_written() == expected_size => staged,
            Ok(staged) => {
                let _ = self.service.blob_storage.discard_staged(&staged).await;
                return Err(CoreError::conflict(format!(
                    "content size mismatch: expected {expected_size}, received {}",
                    staged.bytes_written()
                )));
            }
            Err(error) => {
                let _ = self.service.blob_storage.discard_staged(&staging).await;
                return Err(error);
            }
        };

        self.commit_staged_replacement(
            resource,
            target_key.clone(),
            staged,
            replacement_id,
            backup_key,
            content,
        )
        .await
    }

    async fn commit_staged_replacement(
        &self,
        resource: &mut Resource,
        target_key: StorageKey,
        staged: StagedBlob,
        replacement_id: ResourceContentReplacementId,
        backup_key: StorageKey,
        content: ResourceContent,
    ) -> Result<(), CoreError> {
        let expected_revision = resource.revision();
        if let Err(error) = resource.attach_content(content) {
            let _ = self.service.blob_storage.discard_staged(&staged).await;
            return Err(error.into());
        }
        let replacement = ResourceContentReplacement::rehydrate(
            replacement_id,
            resource.id(),
            expected_revision,
            target_key.clone(),
            staged.key().clone(),
            backup_key.clone(),
            resource
                .content()
                .expect("replacement content was attached")
                .clone(),
        )?;

        let _storage_guard = self.service.storage_key_locks.lock(&target_key).await;
        let current = match self.service.query.find_located_by_id(&resource.id()).await {
            Ok(Some(current)) => current,
            Ok(None) => {
                let _ = self.service.blob_storage.discard_staged(&staged).await;
                return Err(stale_replacement(resource));
            }
            Err(error) => {
                let _ = self.service.blob_storage.discard_staged(&staged).await;
                return Err(error);
            }
        };
        let current_key = match current.storage_key() {
            Ok(current_key) => current_key,
            Err(error) => {
                let _ = self.service.blob_storage.discard_staged(&staged).await;
                return Err(error);
            }
        };
        if current.resource().revision() != expected_revision || current_key != target_key {
            let _ = self.service.blob_storage.discard_staged(&staged).await;
            return Err(stale_replacement(resource));
        }

        if let Err(error) = self.service.content_replacements.save(&replacement).await {
            let _ = self.service.blob_storage.discard_staged(&staged).await;
            return Err(error);
        }

        if let Err(error) = self
            .service
            .blob_storage
            .move_if_absent(&target_key, &backup_key)
            .await
        {
            let _ = self.service.blob_storage.discard_staged(&staged).await;
            let _ = self
                .service
                .content_replacements
                .remove(&replacement.id())
                .await;
            return Err(error);
        }
        if let Err(error) = self
            .service
            .blob_storage
            .publish_staged_if_absent(&staged, &target_key)
            .await
        {
            let restored = self
                .service
                .blob_storage
                .move_if_absent(&backup_key, &target_key)
                .await;
            let _ = self.service.blob_storage.discard_staged(&staged).await;
            if let Err(restore_error) = restored {
                return Err(CoreError::storage(
                    "resource_content_replace.publish_rollback",
                    restore_error,
                ));
            }
            let _ = self
                .service
                .content_replacements
                .remove(&replacement.id())
                .await;
            return Err(error);
        }

        let saved = self
            .service
            .repository
            .save_if_unchanged(resource, expected_revision)
            .await;
        let error = match saved {
            Ok(true) => {
                self.discard_replacement_artifacts(&replacement).await?;
                self.service
                    .content_replacements
                    .remove(&replacement.id())
                    .await?;
                return Ok(());
            }
            Ok(false) => stale_replacement(resource),
            Err(error) => error,
        };

        if let Err(rollback_error) = self.rollback_replacement(&replacement).await {
            return Err(CoreError::storage(
                "resource_content_replace.rollback",
                rollback_error,
            ));
        }
        Err(error)
    }

    pub(crate) async fn resume_pending_replacements(&self) -> Result<usize, CoreError> {
        let replacements = self.service.content_replacements.list_pending().await?;
        let count = replacements.len();
        for replacement in replacements {
            let _storage_guard = self
                .service
                .storage_key_locks
                .lock(replacement.target_key())
                .await;
            self.recover_replacement(&replacement).await?;
        }
        Ok(count)
    }

    async fn recover_replacement(
        &self,
        replacement: &ResourceContentReplacement,
    ) -> Result<(), CoreError> {
        let located = self
            .service
            .query
            .find_located_by_id(&replacement.resource_id())
            .await?
            .ok_or_else(|| {
                CoreError::invariant(format!(
                    "pending content replacement `{}` references a missing resource",
                    replacement.id()
                ))
            })?;
        let current_key = located.storage_key()?;
        let current = located.resource();
        let committed_revision = replacement
            .expected_revision()
            .checked_add(1)
            .ok_or_else(|| CoreError::invariant("resource revision overflow"))?;
        let committed = current_key == *replacement.target_key()
            && current.revision() == committed_revision
            && current.content() == Some(replacement.replacement_content());
        if committed {
            self.discard_replacement_artifacts(replacement).await?;
            self.service
                .content_replacements
                .remove(&replacement.id())
                .await?;
            return Ok(());
        }

        if current_key != *replacement.target_key()
            || current.revision() != replacement.expected_revision()
        {
            return Err(CoreError::conflict(format!(
                "pending content replacement `{}` no longer matches resource `{}`",
                replacement.id(),
                replacement.resource_id()
            )));
        }

        self.rollback_replacement(replacement).await
    }

    async fn rollback_replacement(
        &self,
        replacement: &ResourceContentReplacement,
    ) -> Result<(), CoreError> {
        let backup_exists = self
            .service
            .blob_storage
            .get_stream(replacement.backup_key())
            .await?
            .is_some();
        if backup_exists {
            self.service
                .blob_storage
                .delete(replacement.target_key())
                .await?;
            self.service
                .blob_storage
                .move_if_absent(replacement.backup_key(), replacement.target_key())
                .await?;
        } else if self
            .service
            .blob_storage
            .get_stream(replacement.target_key())
            .await?
            .is_none()
        {
            return Err(CoreError::invariant(format!(
                "pending content replacement `{}` has neither its target nor backup Blob",
                replacement.id()
            )));
        }

        let staged = StagedBlob::new(
            replacement.staged_key().clone(),
            replacement.replacement_content().size(),
        );
        self.service.blob_storage.discard_staged(&staged).await?;
        self.service
            .content_replacements
            .remove(&replacement.id())
            .await
    }

    async fn discard_replacement_artifacts(
        &self,
        replacement: &ResourceContentReplacement,
    ) -> Result<(), CoreError> {
        let staged = StagedBlob::new(
            replacement.staged_key().clone(),
            replacement.replacement_content().size(),
        );
        self.service
            .blob_storage
            .delete(replacement.backup_key())
            .await?;
        self.service.blob_storage.discard_staged(&staged).await
    }
}

fn stale_replacement(resource: &Resource) -> CoreError {
    CoreError::revision_conflict("resource", resource.id().to_string())
}

fn replacement_staging_key(id: ResourceContentReplacementId) -> Result<StorageKey, CoreError> {
    Ok(StorageKey::new(format!(
        "{RESERVED_BLOB_STORAGE_PREFIX}/uploads/replacement-{id}",
    ))?)
}

fn replacement_backup_key(id: ResourceContentReplacementId) -> Result<StorageKey, CoreError> {
    Ok(StorageKey::new(format!(
        "{RESERVED_BLOB_STORAGE_PREFIX}/content-backups/{id}",
    ))?)
}

fn limit_replacement_stream(
    data: BlobByteStream,
    expected_size: u64,
    max_size: u64,
) -> BlobByteStream {
    let mut received = 0_u64;
    Box::pin(data.map(move |chunk| {
        let chunk = chunk?;
        received = received
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| CoreError::invariant("content replacement size overflow"))?;
        if received > expected_size {
            return Err(CoreError::conflict(
                "content replacement exceeds its declared size",
            ));
        }
        if received > max_size {
            return Err(CoreError::limit_exceeded(
                "resource text content",
                max_size,
                received,
            ));
        }
        Ok(chunk)
    }))
}

pub(super) fn build_verified_content(
    size: u64,
    mime_type: Option<String>,
    checksum: Checksum,
    storage_modified_at: Option<DateTime<Utc>>,
) -> Result<ResourceContent, CoreError> {
    let mut content = ResourceContent::verified(size, checksum);
    if let Some(mime_type) = mime_type {
        content = content.with_mime_type(mime_type);
    }
    if let Some(modified_at) = storage_modified_at {
        content = content.with_modified_at(modified_at);
    }
    Ok(content.build()?)
}

pub(super) fn build_pending_content(
    size: u64,
    mime_type: Option<String>,
    storage_modified_at: Option<DateTime<Utc>>,
) -> Result<ResourceContent, CoreError> {
    let mut content = ResourceContent::pending(size);
    if let Some(mime_type) = mime_type {
        content = content.with_mime_type(mime_type);
    }
    if let Some(modified_at) = storage_modified_at {
        content = content.with_modified_at(modified_at);
    }
    Ok(content.build()?)
}

pub(super) fn build_failed_content(
    size: u64,
    mime_type: Option<String>,
    error: impl Into<String>,
    storage_modified_at: Option<DateTime<Utc>>,
) -> Result<ResourceContent, CoreError> {
    let mut content = ResourceContent::verification_failed(size, error);
    if let Some(mime_type) = mime_type {
        content = content.with_mime_type(mime_type);
    }
    if let Some(modified_at) = storage_modified_at {
        content = content.with_modified_at(modified_at);
    }
    Ok(content.build()?)
}

pub(super) fn content_type_for_media(content: &ResourceContent) -> String {
    content
        .mime_type()
        .unwrap_or("application/octet-stream")
        .to_string()
}

const CONTENT_CHECKSUM_KIND: ChecksumKind = ChecksumKind::Sha256;

pub(super) fn calculate_checksum(data: &[u8]) -> Result<Checksum, CoreError> {
    let mut state = ChecksumState::new(CONTENT_CHECKSUM_KIND);
    state.update(data);
    state.finish()
}

pub(super) fn stream_with_checksum_tracking(
    data: BlobByteStream,
) -> (BlobByteStream, Arc<Mutex<ChecksumState>>) {
    let state = Arc::new(Mutex::new(ChecksumState::new(CONTENT_CHECKSUM_KIND)));
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

pub(super) fn finalize_tracked_checksum(
    state: Arc<Mutex<ChecksumState>>,
) -> Result<Checksum, CoreError> {
    state
        .lock()
        .expect("checksum mutex should not be poisoned")
        .finish()
}

pub(super) async fn calculate_stream_checksum(
    stream: BlobByteStream,
) -> Result<Checksum, CoreError> {
    let (mut stream, state) = stream_with_checksum_tracking(stream);
    while let Some(chunk) = stream.next().await {
        chunk?;
    }
    finalize_tracked_checksum(state)
}

pub(super) enum ChecksumState {
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
pub(super) fn hex_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex_digest(&hasher.finalize())
}

pub(super) fn hex_digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}
