//! 资源内容的上传与读取。
//!
//! 本模块只处理资源内容引用与 Blob 之间的编排；后台扫描协调位于 `reconciliation`。

use super::{ResourceContentStream, StorageKeyLocks, path_resolver};
use crate::CoreError;
use crate::{
    resource::{
        domain::{
            Checksum, ChecksumKind, Resource, ResourceContent, ResourceContentReplacement,
            ResourceContentReplacementId, ResourceId, UploadSession,
        },
        port::{ResourceContentReplacementStore, ResourceReadModel, ResourceStore},
        query::LocatedResource,
    },
    storage::StorageKey,
    storage::port::{
        BlobByteStream, ContentObjectStore, ContentReader, ContentStagingStore, StagedBlob,
    },
};
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::{
    ops::Range,
    sync::{Arc, Mutex},
};

/// Process-lifecycle recovery for interrupted content replacements.
#[derive(Clone)]
pub struct ContentRecoveryService {
    content: ContentService,
}

impl ContentRecoveryService {
    pub(super) fn new(content: ContentService) -> Self {
        Self { content }
    }

    pub async fn resume_pending_replacements(&self) -> Result<usize, CoreError> {
        self.content.resume_pending_replacements().await
    }
}

/// Resource-content reads, streams, and replacement workflows.
#[derive(Clone)]
pub struct ContentService {
    read_model: Arc<dyn ResourceReadModel>,
    store: Arc<dyn ResourceStore>,
    reader: Arc<dyn ContentReader>,
    staging: Arc<dyn ContentStagingStore>,
    objects: Arc<dyn ContentObjectStore>,
    content_replacements: Arc<dyn ResourceContentReplacementStore>,
    storage_key_locks: Arc<StorageKeyLocks>,
}

impl ContentService {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        read_model: Arc<dyn ResourceReadModel>,
        store: Arc<dyn ResourceStore>,
        reader: Arc<dyn ContentReader>,
        staging: Arc<dyn ContentStagingStore>,
        objects: Arc<dyn ContentObjectStore>,
        content_replacements: Arc<dyn ResourceContentReplacementStore>,
        storage_key_locks: Arc<StorageKeyLocks>,
    ) -> Self {
        Self {
            read_model,
            store,
            reader,
            staging,
            objects,
            content_replacements,
            storage_key_locks,
        }
    }

    /// Stream Resource content by stable ID, optionally constrained to a half-open byte range.
    pub async fn stream(
        &self,
        id: &ResourceId,
        range: Option<Range<u64>>,
    ) -> Result<Option<ResourceContentStream>, CoreError> {
        let Some(resource) = self.read_model.find_by_id(id).await? else {
            return Ok(None);
        };
        self.get_resource_content_stream_snapshot(&resource, range)
            .await
    }

    async fn get_resource_content_stream_snapshot(
        &self,
        located: &LocatedResource,
        range: Option<Range<u64>>,
    ) -> Result<Option<ResourceContentStream>, CoreError> {
        let resource = located.resource();
        let Some(content) = resource.content() else {
            return Ok(None);
        };

        let storage_key = located.storage_key()?;
        let stream = if let Some(range) = range {
            self.reader.get_range_stream(&storage_key, range).await?
        } else {
            self.reader.get_stream(&storage_key).await?
        };

        Ok(stream.map(|content_stream| {
            ResourceContentStream::new(
                content_type_for_media(content),
                content.size(),
                content_stream,
            )
        }))
    }

    pub(super) async fn commit_replacement_upload(
        &self,
        session: &UploadSession,
        staged: StagedBlob,
        checksum: Checksum,
    ) -> Result<Resource, CoreError> {
        let expected_revision = session.purpose().expected_revision().ok_or_else(|| {
            CoreError::invariant("content replacement requires a replacement upload session")
        })?;
        let located = self
            .read_model
            .find_by_id(&session.resource_id())
            .await?
            .ok_or_else(|| CoreError::not_found("resource", session.resource_id().to_string()))?;
        let (mut resource, directory) = located.into_parts();
        if resource.revision() != expected_revision {
            return Err(stale_replacement(&resource));
        }
        if resource.content().is_none() {
            return Err(CoreError::invalid_operation(
                "resource content replacement requires existing content",
            ));
        }
        let target_key = path_resolver::resource_key(&directory, resource.name())?;
        let replacement_id = ResourceContentReplacementId::new();
        let backup_key = path_resolver::replacement_backup_key(replacement_id)?;
        let content = build_verified_content(
            session.expected_size(),
            session.mime_type().map(str::to_string),
            checksum,
            None,
        )?;
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
            let _ = self.staging.discard_staged(&staged).await;
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

        let _storage_guard = self.storage_key_locks.lock(&target_key).await;
        let current = match self.read_model.find_by_id(&resource.id()).await {
            Ok(Some(current)) => current,
            Ok(None) => {
                let _ = self.staging.discard_staged(&staged).await;
                return Err(stale_replacement(resource));
            }
            Err(error) => {
                let _ = self.staging.discard_staged(&staged).await;
                return Err(error);
            }
        };
        let current_key = match current.storage_key() {
            Ok(current_key) => current_key,
            Err(error) => {
                let _ = self.staging.discard_staged(&staged).await;
                return Err(error);
            }
        };
        if current.resource().revision() != expected_revision || current_key != target_key {
            let _ = self.staging.discard_staged(&staged).await;
            return Err(stale_replacement(resource));
        }

        if let Err(error) = self.content_replacements.save(&replacement).await {
            let _ = self.staging.discard_staged(&staged).await;
            return Err(error);
        }

        if let Err(error) = self.objects.move_if_absent(&target_key, &backup_key).await {
            let _ = self.staging.discard_staged(&staged).await;
            let _ = self.content_replacements.remove(&replacement.id()).await;
            return Err(error);
        }
        if let Err(error) = self
            .staging
            .publish_staged_if_absent(&staged, &target_key)
            .await
        {
            let restored = self.objects.move_if_absent(&backup_key, &target_key).await;
            let _ = self.staging.discard_staged(&staged).await;
            if let Err(restore_error) = restored {
                return Err(CoreError::storage(
                    "resource_content_replace.publish_rollback",
                    restore_error,
                ));
            }
            let _ = self.content_replacements.remove(&replacement.id()).await;
            return Err(error);
        }

        let saved = self
            .store
            .update_if_revision(resource, expected_revision)
            .await;
        let error = match saved {
            Ok(true) => {
                self.discard_replacement_artifacts(&replacement).await?;
                self.content_replacements.remove(&replacement.id()).await?;
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

    async fn resume_pending_replacements(&self) -> Result<usize, CoreError> {
        let replacements = self.content_replacements.list_pending().await?;
        let count = replacements.len();
        for replacement in replacements {
            let _storage_guard = self.storage_key_locks.lock(replacement.target_key()).await;
            self.recover_replacement(&replacement).await?;
        }
        Ok(count)
    }

    async fn recover_replacement(
        &self,
        replacement: &ResourceContentReplacement,
    ) -> Result<(), CoreError> {
        let located = self
            .read_model
            .find_by_id(&replacement.resource_id())
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
            self.content_replacements.remove(&replacement.id()).await?;
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
        let backup_exists = self.objects.exists(replacement.backup_key()).await?;
        if backup_exists {
            self.objects.delete(replacement.target_key()).await?;
            self.objects
                .move_if_absent(replacement.backup_key(), replacement.target_key())
                .await?;
        } else if !self.objects.exists(replacement.target_key()).await? {
            return Err(CoreError::invariant(format!(
                "pending content replacement `{}` has neither its target nor backup Blob",
                replacement.id()
            )));
        }

        let staged = StagedBlob::new(
            replacement.staged_key().clone(),
            replacement.replacement_content().size(),
        );
        self.staging.discard_staged(&staged).await?;
        self.content_replacements.remove(&replacement.id()).await
    }

    async fn discard_replacement_artifacts(
        &self,
        replacement: &ResourceContentReplacement,
    ) -> Result<(), CoreError> {
        let staged = StagedBlob::new(
            replacement.staged_key().clone(),
            replacement.replacement_content().size(),
        );
        self.objects.delete(replacement.backup_key()).await?;
        self.staging.discard_staged(&staged).await
    }
}

fn stale_replacement(resource: &Resource) -> CoreError {
    CoreError::revision_conflict("resource", resource.id().to_string())
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

pub(super) fn hex_digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}
