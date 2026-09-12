use super::command::build_resource;
use super::content::{
    build_verified_content, calculate_stream_checksum, finalize_tracked_checksum,
    stream_with_checksum_tracking,
};
use super::{
    ContentService, CreateContentReplacementUpload, CreateUpload, MAX_UPLOAD_CHUNK_SIZE,
    StorageKeyLocks, UploadLocks, path_resolver,
};
use crate::CoreError;
use crate::{
    directory::service::DirectoryService,
    idempotency::{
        domain::IdempotencyKey,
        service::{IdempotencyOutcome, IdempotencyService, request_hash},
    },
    resource::{
        domain::{
            Checksum, Resource, ResourceId, UploadId, UploadPurpose, UploadSession, UploadStatus,
        },
        port::{ResourceReadModel, ResourceStore, UploadSessionStore},
    },
    storage::port::{
        BlobByteStream, ContentObjectStore, ContentReader, ContentStagingStore, StagedBlob,
        StorageScanner,
    },
    storage::{RESERVED_BLOB_STORAGE_PREFIX, StorageKey},
};
use futures_util::StreamExt;
use std::sync::Arc;

/// Background finalization operations for uploads already in `Finalizing` state.
///
/// Runtime owns scheduling; this handle shares the ordinary upload service's locks and durable
/// session repository rather than rebuilding a second upload service.
#[derive(Clone)]
pub struct UploadFinalizationService {
    uploads: UploadService,
}

impl UploadFinalizationService {
    pub(super) fn new(uploads: UploadService) -> Self {
        Self { uploads }
    }

    pub async fn pending_finalizations(&self) -> Result<Vec<UploadId>, CoreError> {
        self.uploads.pending_finalizations().await
    }

    pub async fn finalize(&self, id: &UploadId) -> Result<Resource, CoreError> {
        self.uploads.finalize(id).await
    }
}

/// Resumable upload workflows with durable state and recovery.
#[derive(Clone)]
pub struct UploadService {
    service: Arc<UploadDependencies>,
}

struct UploadDependencies {
    store: Arc<dyn ResourceStore>,
    read_model: Arc<dyn ResourceReadModel>,
    staging: Arc<dyn ContentStagingStore>,
    reader: Arc<dyn ContentReader>,
    objects: Arc<dyn ContentObjectStore>,
    storage_scanner: Arc<dyn StorageScanner>,
    directories: DirectoryService,
    upload_sessions: Arc<dyn UploadSessionStore>,
    storage_key_locks: Arc<StorageKeyLocks>,
    upload_locks: Arc<UploadLocks>,
    content: ContentService,
    idempotency: IdempotencyService,
}

impl UploadService {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        store: Arc<dyn ResourceStore>,
        read_model: Arc<dyn ResourceReadModel>,
        staging: Arc<dyn ContentStagingStore>,
        reader: Arc<dyn ContentReader>,
        objects: Arc<dyn ContentObjectStore>,
        storage_scanner: Arc<dyn StorageScanner>,
        directories: DirectoryService,
        upload_sessions: Arc<dyn UploadSessionStore>,
        storage_key_locks: Arc<StorageKeyLocks>,
        content: ContentService,
        idempotency: IdempotencyService,
    ) -> Self {
        Self {
            service: Arc::new(UploadDependencies {
                store,
                read_model,
                staging,
                reader,
                objects,
                storage_scanner,
                directories,
                upload_sessions,
                storage_key_locks,
                upload_locks: Arc::new(UploadLocks::default()),
                content,
                idempotency,
            }),
        }
    }

    async fn pending_finalizations(&self) -> Result<Vec<UploadId>, CoreError> {
        self.service.upload_sessions.list_finalizing().await
    }

    /// Create a resumable upload session for the requested resource and target directory.
    pub async fn create(&self, command: CreateUpload) -> Result<UploadSession, CoreError> {
        let Some(key) = command.idempotency_key().cloned() else {
            return self.create_session(command, None).await;
        };
        let hash = request_hash(&serde_json::json!({
            "operation": "create_resource",
            "name": &command.name,
            "directory_id": command.directory_id.to_string(),
            "mime_type": &command.mime_type,
            "expected_size": command.expected_size,
            "expected_checksum": command.expected_checksum.value(),
        }));
        match self.service.idempotency.begin(&key, &hash).await? {
            IdempotencyOutcome::Acquired { execution_id } => {
                match self
                    .service
                    .idempotency
                    .execute_with_lease(
                        &key,
                        execution_id,
                        self.create_session(command, Some(&key)),
                    )
                    .await
                {
                    Ok(session) => {
                        self.service
                            .idempotency
                            .complete(
                                &key,
                                execution_id,
                                serde_json::json!({ "upload_id": session.id().to_string() }),
                            )
                            .await?;
                        Ok(session)
                    }
                    Err(error) => {
                        self.service.idempotency.abandon(&key, execution_id).await?;
                        Err(error)
                    }
                }
            }
            IdempotencyOutcome::Replay(result) => self.replay_upload(&result).await,
            IdempotencyOutcome::ConflictDifferentRequest => Err(CoreError::conflict(format!(
                "idempotency key `{key}` was already used for a different request"
            ))),
            IdempotencyOutcome::AlreadyInProgress => Err(CoreError::conflict(format!(
                "idempotency key `{key}` is currently executing"
            ))),
        }
    }

    /// 为已有资源创建可恢复、可分片的内容替换会话。
    pub async fn create_content_replacement(
        &self,
        resource_id: &ResourceId,
        command: CreateContentReplacementUpload,
    ) -> Result<Option<UploadSession>, CoreError> {
        let Some(located) = self.service.read_model.find_by_id(resource_id).await? else {
            return Ok(None);
        };
        let Some(key) = command.idempotency_key().cloned() else {
            return self
                .create_replacement_session(located, command, None)
                .await
                .map(Some);
        };
        let hash = request_hash(&serde_json::json!({
            "operation": "replace_resource_content",
            "resource_id": resource_id.to_string(),
            "expected_size": command.expected_size,
            "expected_checksum": command.expected_checksum.value(),
            "expected_revision": command.expected_revision,
            "mime_type": &command.mime_type,
        }));
        let session = match self.service.idempotency.begin(&key, &hash).await? {
            IdempotencyOutcome::Acquired { execution_id } => {
                match self
                    .service
                    .idempotency
                    .execute_with_lease(
                        &key,
                        execution_id,
                        self.create_replacement_session(located, command, Some(&key)),
                    )
                    .await
                {
                    Ok(session) => {
                        self.service
                            .idempotency
                            .complete(
                                &key,
                                execution_id,
                                serde_json::json!({ "upload_id": session.id().to_string() }),
                            )
                            .await?;
                        session
                    }
                    Err(error) => {
                        self.service.idempotency.abandon(&key, execution_id).await?;
                        return Err(error);
                    }
                }
            }
            IdempotencyOutcome::Replay(result) => self.replay_upload(&result).await?,
            IdempotencyOutcome::ConflictDifferentRequest => {
                return Err(CoreError::conflict(format!(
                    "idempotency key `{key}` was already used for a different request"
                )));
            }
            IdempotencyOutcome::AlreadyInProgress => {
                return Err(CoreError::conflict(format!(
                    "idempotency key `{key}` is currently executing"
                )));
            }
        };
        Ok(Some(session))
    }

    async fn create_session(
        &self,
        command: CreateUpload,
        idempotency_key: Option<&IdempotencyKey>,
    ) -> Result<UploadSession, CoreError> {
        if let Some(key) = idempotency_key
            && let Some(session) = self
                .service
                .upload_sessions
                .find_by_idempotency_key(key)
                .await?
        {
            return Ok(session);
        }
        let CreateUpload {
            name,
            directory_id,
            mime_type,
            expected_size,
            expected_checksum,
            ..
        } = command;
        let directory = self.service.directories.find_by_id(&directory_id).await?;
        let storage_key = path_resolver::resource_key(directory.path(), &name)?;
        reject_reserved_storage_key(&storage_key)?;
        build_resource(name.clone(), directory.id()).build()?;
        if self
            .service
            .read_model
            .find_by_directory_and_name(directory.id(), &name)
            .await?
            .is_some()
        {
            return Err(CoreError::conflict(format!(
                "resource path `{storage_key}` already exists"
            )));
        }

        let session = UploadSession::for_resource_creation(
            name,
            directory.id(),
            mime_type,
            expected_size,
            expected_checksum,
        )?;
        self.persist_new_session(session, idempotency_key).await
    }

    async fn create_replacement_session(
        &self,
        located: crate::resource::query::LocatedResource,
        command: CreateContentReplacementUpload,
        idempotency_key: Option<&IdempotencyKey>,
    ) -> Result<UploadSession, CoreError> {
        if let Some(key) = idempotency_key
            && let Some(session) = self
                .service
                .upload_sessions
                .find_by_idempotency_key(key)
                .await?
        {
            return Ok(session);
        }
        let CreateContentReplacementUpload {
            expected_size,
            expected_checksum,
            expected_revision,
            mime_type,
            ..
        } = command;
        let resource = located.resource();
        if resource.revision() != expected_revision {
            return Err(CoreError::revision_conflict(
                "resource",
                resource.id().to_string(),
            ));
        }
        let current_content = resource.content().ok_or_else(|| {
            CoreError::invalid_operation("resource content replacement requires existing content")
        })?;
        let session = UploadSession::for_content_replacement(
            resource.id(),
            expected_revision,
            resource.name(),
            resource.directory_id(),
            mime_type.or_else(|| current_content.mime_type().map(str::to_string)),
            expected_size,
            expected_checksum,
        )?;
        self.persist_new_session(session, idempotency_key).await
    }

    async fn persist_new_session(
        &self,
        session: UploadSession,
        idempotency_key: Option<&IdempotencyKey>,
    ) -> Result<UploadSession, CoreError> {
        let staged = staged_for(session.id())?;
        self.service.staging.create_staged(staged.key()).await?;
        let save = match idempotency_key {
            Some(key) => {
                self.service
                    .upload_sessions
                    .save_with_idempotency_key(&session, key)
                    .await
            }
            None => self.service.upload_sessions.save(&session).await,
        };
        if let Err(error) = save {
            let _ = self.service.staging.discard_staged(&staged).await;
            return Err(error);
        }
        Ok(session)
    }

    async fn replay_upload(&self, result: &serde_json::Value) -> Result<UploadSession, CoreError> {
        let upload_id = result
            .get("upload_id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| CoreError::invariant("idempotency result is missing `upload_id`"))?;
        let upload_id = std::str::FromStr::from_str(upload_id)
            .map_err(|error| CoreError::invariant(format!("invalid stored upload id: {error}")))?;
        self.load(&upload_id).await
    }

    pub async fn status(&self, id: &UploadId) -> Result<UploadSession, CoreError> {
        let _guard = self.service.upload_locks.lock(id).await;
        let session = self.load(id).await?;
        if session.status() != UploadStatus::Uploading {
            return Ok(session);
        }
        self.sync_offset(session).await
    }

    pub async fn append(
        &self,
        id: &UploadId,
        requested_offset: u64,
        expected_chunk_checksum: Checksum,
        data: BlobByteStream,
    ) -> Result<UploadSession, CoreError> {
        let _guard = self.service.upload_locks.lock(id).await;
        let session = self.load(id).await?;
        if session.status() != UploadStatus::Uploading {
            return Err(CoreError::conflict(format!(
                "upload is not accepting chunks while its status is `{}`",
                session.status().as_str()
            )));
        }
        let mut session = self.sync_offset(session).await?;
        if requested_offset != session.offset() {
            return Err(CoreError::conflict(format!(
                "upload offset mismatch: expected {}, received {requested_offset}",
                session.offset()
            )));
        }
        let remaining = session
            .expected_size()
            .checked_sub(session.offset())
            .ok_or_else(|| CoreError::conflict("upload offset exceeds expected size"))?;
        let staged_key = staged_for(*id)?;
        let chunk_key = chunk_for(*id)?;
        self.service.staging.discard_staged(&chunk_key).await?;
        let chunk = self.service.staging.create_staged(chunk_key.key()).await?;

        let append_result = async {
            let (tracked_data, checksum_state) =
                stream_with_checksum_tracking(limit_stream(data, remaining));
            self.service
                .staging
                .append_staged(chunk.key(), 0, tracked_data)
                .await?;
            let actual_chunk_checksum = finalize_tracked_checksum(checksum_state)?;
            if actual_chunk_checksum != expected_chunk_checksum {
                return Err(CoreError::conflict(format!(
                    "upload chunk checksum mismatch: expected {}, actual {}",
                    expected_chunk_checksum.value(),
                    actual_chunk_checksum.value()
                )));
            }

            let verified_chunk = self
                .service
                .reader
                .get_stream(chunk.key())
                .await?
                .ok_or_else(|| CoreError::not_found("staged upload chunk", id.to_string()))?;
            let staged = self
                .service
                .staging
                .append_staged(staged_key.key(), session.offset(), verified_chunk)
                .await?;
            if !self
                .service
                .upload_sessions
                .update_offset(id, session.offset(), staged.bytes_written())
                .await?
            {
                return Err(CoreError::conflict(
                    "upload session offset changed concurrently",
                ));
            }
            Ok(staged.bytes_written())
        }
        .await;

        let cleanup_result = self.service.staging.discard_staged(&chunk).await;
        let offset = append_result?;
        cleanup_result?;
        session.synchronize_offset(offset)?;
        Ok(session)
    }

    pub async fn request_finalization(
        &self,
        id: &UploadId,
    ) -> Result<(UploadSession, bool), CoreError> {
        let _upload_guard = self.service.upload_locks.lock(id).await;
        let session = self.load(id).await?;
        if matches!(
            session.status(),
            UploadStatus::Finalizing | UploadStatus::Completed
        ) {
            return Ok((session, false));
        }
        let session = self.sync_offset(session).await?;
        if session.offset() != session.expected_size() {
            return Err(CoreError::conflict(format!(
                "upload is incomplete: expected {} bytes, received {}",
                session.expected_size(),
                session.offset()
            )));
        }
        self.service
            .staging
            .discard_staged(&chunk_for(*id)?)
            .await?;
        if !self.service.upload_sessions.mark_finalizing(id).await? {
            return Err(CoreError::conflict(
                "upload session status changed concurrently",
            ));
        }
        let mut session = session;
        session.mark_finalizing()?;
        Ok((session, true))
    }

    async fn finalize(&self, id: &UploadId) -> Result<Resource, CoreError> {
        let _upload_guard = self.service.upload_locks.lock(id).await;
        let mut session = self.load(id).await?;
        if session.status() == UploadStatus::Completed {
            return self.completed_resource(&session).await;
        }
        if session.status() != UploadStatus::Finalizing {
            return Err(CoreError::conflict(format!(
                "upload cannot be finalized while its status is `{}`",
                session.status().as_str()
            )));
        }

        let result = self.finalize_session(&mut session).await;
        if let Err(error) = &result {
            let failure = error.to_string();
            if let Err(persistence_error) =
                self.service.upload_sessions.mark_failed(id, &failure).await
            {
                tracing::error!(
                    upload_id = %id,
                    error = %persistence_error,
                    "failed to persist upload finalization failure"
                );
            }
        }
        result
    }

    async fn finalize_session(&self, session: &mut UploadSession) -> Result<Resource, CoreError> {
        match session.purpose() {
            UploadPurpose::CreateResource => self.finalize_resource_creation(session).await,
            UploadPurpose::ReplaceContent { .. } => {
                self.finalize_content_replacement(session).await
            }
        }
    }

    async fn finalize_resource_creation(
        &self,
        session: &mut UploadSession,
    ) -> Result<Resource, CoreError> {
        let id = session.id();
        if let Some(resource) = self.service.store.load(&session.resource_id()).await? {
            self.service.upload_sessions.mark_completed(&id).await?;
            let _ = self.service.staging.discard_staged(&staged_for(id)?).await;
            return Ok(resource);
        }

        let staged = staged_for(id)?;
        let checksum = match session.actual_checksum() {
            Some(checksum) => checksum.clone(),
            None => {
                let checksum_stream = self
                    .service
                    .reader
                    .get_stream(staged.key())
                    .await?
                    .ok_or_else(|| CoreError::not_found("staged upload", id.to_string()))?;
                let checksum = calculate_stream_checksum(checksum_stream).await?;
                self.service
                    .upload_sessions
                    .save_actual_checksum(&id, &checksum)
                    .await?;
                session.set_actual_checksum(checksum.clone())?;
                checksum
            }
        };
        if checksum != *session.expected_checksum() {
            return Err(CoreError::conflict(format!(
                "upload checksum mismatch: expected {}, actual {}",
                session.expected_checksum().value(),
                checksum.value()
            )));
        }
        let directory = self
            .service
            .directories
            .find_by_id(&session.directory_id())
            .await?;
        let mut resource = build_resource(session.name().to_string(), directory.id())
            .with_id(session.resource_id())
            .build()?;
        let storage_key = path_resolver::resource_key(directory.path(), session.name())?;

        let _storage_guard = self.service.storage_key_locks.lock(&storage_key).await;
        if let Some(existing) = self
            .service
            .read_model
            .find_by_directory_and_name(directory.id(), session.name())
            .await?
        {
            if existing.resource().id() == session.resource_id() {
                self.service.upload_sessions.mark_completed(&id).await?;
                let _ = self.service.staging.discard_staged(&staged).await;
                return Ok(existing.into_resource());
            }
            return Err(CoreError::conflict(format!(
                "resource path `{storage_key}` already exists"
            )));
        }

        let published = match self
            .service
            .staging
            .publish_staged_if_absent(&staged, &storage_key)
            .await
        {
            Ok(()) => true,
            Err(CoreError::Conflict { .. })
                if self
                    .published_target_matches(&storage_key, session.offset(), &checksum)
                    .await? =>
            {
                false
            }
            Err(error) => return Err(error),
        };

        let finalized = async {
            let stored = self
                .service
                .storage_scanner
                .inspect(&storage_key)
                .await?
                .ok_or_else(|| {
                    CoreError::conflict(format!(
                        "blob `{storage_key}` disappeared while upload was finalized"
                    ))
                })?;
            if stored.size != session.offset() {
                return Err(CoreError::conflict(format!(
                    "blob `{storage_key}` changed while upload was finalized"
                )));
            }
            resource.attach_content(build_verified_content(
                stored.size,
                session.mime_type().map(str::to_string),
                checksum.clone(),
                Some(stored.modified_at),
            )?)?;
            self.service.store.insert(&resource).await?;
            if let Err(error) = self.service.upload_sessions.mark_completed(&id).await {
                let _ = self
                    .service
                    .store
                    .delete_if_revision(&resource.id(), resource.revision())
                    .await;
                return Err(error);
            }
            Ok(resource)
        }
        .await;

        match finalized {
            Ok(resource) => {
                let _ = self.service.staging.discard_staged(&staged).await;
                Ok(resource)
            }
            Err(error) => {
                if published || session.status() == UploadStatus::Finalizing {
                    let _ = self.service.objects.delete(&storage_key).await;
                }
                Err(error)
            }
        }
    }

    async fn finalize_content_replacement(
        &self,
        session: &mut UploadSession,
    ) -> Result<Resource, CoreError> {
        let id = session.id();
        let expected_revision = session.purpose().expected_revision().ok_or_else(|| {
            CoreError::invariant("replacement finalization requires an expected revision")
        })?;
        let completed_revision = expected_revision
            .checked_add(1)
            .ok_or_else(|| CoreError::invariant("resource revision overflow"))?;
        if let Some(resource) = self.service.store.load(&session.resource_id()).await?
            && resource.revision() == completed_revision
            && resource.content().is_some_and(|content| {
                content.size() == session.expected_size()
                    && content.checksum() == Some(session.expected_checksum())
                    && content.mime_type() == session.mime_type()
            })
        {
            self.service.upload_sessions.mark_completed(&id).await?;
            let _ = self.service.staging.discard_staged(&staged_for(id)?).await;
            return Ok(resource);
        }

        let staged = staged_for(id)?;
        let checksum = match session.actual_checksum() {
            Some(checksum) => checksum.clone(),
            None => {
                let checksum_stream = self
                    .service
                    .reader
                    .get_stream(staged.key())
                    .await?
                    .ok_or_else(|| CoreError::not_found("staged upload", id.to_string()))?;
                let checksum = calculate_stream_checksum(checksum_stream).await?;
                self.service
                    .upload_sessions
                    .save_actual_checksum(&id, &checksum)
                    .await?;
                session.set_actual_checksum(checksum.clone())?;
                checksum
            }
        };
        if checksum != *session.expected_checksum() {
            return Err(CoreError::conflict(format!(
                "upload checksum mismatch: expected {}, actual {}",
                session.expected_checksum().value(),
                checksum.value()
            )));
        }

        let resource = self
            .service
            .content
            .commit_replacement_upload(session, staged, checksum)
            .await?;
        self.service.upload_sessions.mark_completed(&id).await?;
        Ok(resource)
    }

    pub async fn abort(&self, id: &UploadId) -> Result<(), CoreError> {
        let _guard = self.service.upload_locks.lock(id).await;
        let session = self.load(id).await?;
        let staged = staged_for(session.id())?;
        let chunk = chunk_for(session.id())?;
        self.service.staging.discard_staged(&chunk).await?;
        self.service.staging.discard_staged(&staged).await?;
        self.service.upload_sessions.remove(id).await
    }

    async fn load(&self, id: &UploadId) -> Result<UploadSession, CoreError> {
        self.service
            .upload_sessions
            .find_by_id(id)
            .await?
            .ok_or_else(|| CoreError::not_found("upload", id.to_string()))
    }

    async fn completed_resource(&self, session: &UploadSession) -> Result<Resource, CoreError> {
        self.service
            .store
            .load(&session.resource_id())
            .await?
            .ok_or_else(|| CoreError::not_found("resource", session.resource_id().to_string()))
    }

    async fn published_target_matches(
        &self,
        storage_key: &StorageKey,
        expected_size: u64,
        expected_checksum: &Checksum,
    ) -> Result<bool, CoreError> {
        let Some(stored) = self.service.storage_scanner.inspect(storage_key).await? else {
            return Ok(false);
        };
        if stored.size != expected_size {
            return Ok(false);
        }
        let Some(stream) = self.service.reader.get_stream(storage_key).await? else {
            return Ok(false);
        };
        Ok(calculate_stream_checksum(stream).await? == *expected_checksum)
    }

    async fn sync_offset(&self, mut session: UploadSession) -> Result<UploadSession, CoreError> {
        let id = session.id();
        let actual = self
            .service
            .staging
            .inspect_staged(staged_for(id)?.key())
            .await?
            .ok_or_else(|| CoreError::not_found("staged upload", id.to_string()))?
            .bytes_written();
        if actual > session.expected_size() {
            return Err(CoreError::conflict(
                "staged upload exceeds its declared size",
            ));
        }
        if actual != session.offset() {
            if !self
                .service
                .upload_sessions
                .update_offset(&id, session.offset(), actual)
                .await?
            {
                return Err(CoreError::conflict(
                    "upload session offset changed concurrently",
                ));
            }
            session.synchronize_offset(actual)?;
        }
        Ok(session)
    }
}

fn staged_for(id: UploadId) -> Result<StagedBlob, CoreError> {
    Ok(StagedBlob::new(path_resolver::upload_staging_key(id)?, 0))
}

fn chunk_for(id: UploadId) -> Result<StagedBlob, CoreError> {
    Ok(StagedBlob::new(path_resolver::upload_chunk_key(id)?, 0))
}

fn reject_reserved_storage_key(key: &StorageKey) -> Result<(), CoreError> {
    if key.as_str() == RESERVED_BLOB_STORAGE_PREFIX
        || key
            .as_str()
            .starts_with(&format!("{RESERVED_BLOB_STORAGE_PREFIX}/"))
    {
        return Err(CoreError::invalid_operation(format!(
            "storage key `{key}` uses reserved Asset Hub namespace"
        )));
    }
    Ok(())
}

fn limit_stream(data: BlobByteStream, remaining: u64) -> BlobByteStream {
    let mut received = 0_u64;
    Box::pin(data.map(move |chunk| {
        let chunk = chunk?;
        received = received
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| CoreError::invariant("upload size overflow"))?;
        if received > remaining {
            return Err(CoreError::conflict(
                "upload chunk exceeds the declared upload size",
            ));
        }
        if received > MAX_UPLOAD_CHUNK_SIZE {
            return Err(CoreError::limit_exceeded(
                "upload chunk",
                MAX_UPLOAD_CHUNK_SIZE,
                received,
            ));
        }
        Ok(chunk)
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use futures_util::{StreamExt, stream};

    #[tokio::test]
    async fn rejects_a_chunk_larger_than_the_fixed_protocol_limit() {
        let data: BlobByteStream = Box::pin(stream::iter([
            Ok(Bytes::from(vec![0; MAX_UPLOAD_CHUNK_SIZE as usize])),
            Ok(Bytes::from_static(&[0])),
        ]));
        let mut limited = limit_stream(data, MAX_UPLOAD_CHUNK_SIZE + 1);

        assert!(limited.next().await.unwrap().is_ok());
        let error = limited.next().await.unwrap().unwrap_err();
        assert!(matches!(
            error,
            CoreError::LimitExceeded {
                resource: "upload chunk",
                limit: MAX_UPLOAD_CHUNK_SIZE,
                actual
            } if actual == MAX_UPLOAD_CHUNK_SIZE + 1
        ));
    }
}
