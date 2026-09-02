use super::command::build_resource;
use super::content::{
    build_verified_content, calculate_stream_checksum, finalize_tracked_checksum,
    stream_with_checksum_tracking,
};
use super::{CreateUpload, StorageKeyLocks, UploadLocks};
use crate::CoreError;
use crate::domain::{
    AccessContext, Checksum, DirectoryOperation, Resource, ResourceKind, StorageKey, UploadId,
    UploadSession, UploadStatus, UserId,
};
use crate::port::{
    BlobByteStream, BlobStorage, ResourceKindRegistry, ResourceReadModel, ResourceStore,
    StorageScanner, UploadSessionRepository, RESERVED_BLOB_STORAGE_PREFIX, StagedBlob,
};
use crate::service::{AuthorizationService, DirectoryService};
use futures_util::StreamExt;
use bytes::Bytes;
use sha2::{Digest, Sha256};
use std::sync::Arc;

#[derive(Clone)]
pub struct UploadService {
    service: Arc<UploadDependencies>,
}

struct UploadDependencies {
    store: Arc<dyn ResourceStore>,
    read_model: Arc<dyn ResourceReadModel>,
    blob_storage: Arc<dyn BlobStorage>,
    storage_scanner: Arc<dyn StorageScanner>,
    directories: DirectoryService,
    kind_registry: Arc<dyn ResourceKindRegistry>,
    upload_sessions: Arc<dyn UploadSessionRepository>,
    storage_key_locks: Arc<StorageKeyLocks>,
    upload_locks: Arc<UploadLocks>,
}

impl UploadDependencies {
    fn resolve_content_kind(
        &self,
        kind: Option<ResourceKind>,
        mime_type: Option<&str>,
        storage_key: Option<&str>,
    ) -> Result<ResourceKind, CoreError> {
        let kind = match kind {
            Some(kind) => kind,
            None => self
                .kind_registry
                .detect_content_kind(mime_type, storage_key)?
                .unwrap_or_default(),
        };
        let definition = self
            .kind_registry
            .get(&kind)
            .ok_or_else(|| CoreError::unsupported("resource kind", kind.to_string()))?;
        if !definition.supports_content() {
            return Err(CoreError::unsupported(
                "resource kind for content upload",
                kind.to_string(),
            ));
        }
        Ok(kind)
    }
}

impl UploadService {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        store: Arc<dyn ResourceStore>,
        read_model: Arc<dyn ResourceReadModel>,
        blob_storage: Arc<dyn BlobStorage>,
        storage_scanner: Arc<dyn StorageScanner>,
        directories: DirectoryService,
        kind_registry: Arc<dyn ResourceKindRegistry>,
        upload_sessions: Arc<dyn UploadSessionRepository>,
        storage_key_locks: Arc<StorageKeyLocks>,
    ) -> Self {
        Self {
            service: Arc::new(UploadDependencies {
                store,
                read_model,
                blob_storage,
                storage_scanner,
                directories,
                kind_registry,
                upload_sessions,
                storage_key_locks,
                upload_locks: Arc::new(UploadLocks::default()),
            }),
        }
    }

    pub fn secured<'a>(
        &'a self,
        authorization: &'a AuthorizationService,
        context: &'a AccessContext,
    ) -> SecuredUploadService<'a> {
        SecuredUploadService {
            service: self,
            authorization,
            context,
        }
    }

    pub async fn pending_finalizations(&self) -> Result<Vec<UploadId>, CoreError> {
        self.service.upload_sessions.list_finalizing().await
    }

    pub(crate) async fn create_generated(
        &self,
        directory: &crate::port::DirectoryLocation,
        name: String,
        kind: Option<ResourceKind>,
        mime_type: Option<String>,
        data: Bytes,
    ) -> Result<crate::port::LocatedResource, CoreError> {
        let storage_key = StorageKey::from_resource_path(directory.path(), &name)?;
        let kind = self.service.resolve_content_kind(
            kind,
            mime_type.as_deref(),
            Some(storage_key.as_str()),
        )?;
        let mut resource = build_resource(name.clone(), directory.id(), Some(kind)).build()?;
        let checksum = Checksum::sha256(super::content::hex_digest(&Sha256::digest(&data)))?;
        resource.attach_content(build_verified_content(
            data.len() as u64,
            mime_type,
            checksum,
            None,
        )?)?;
        let _guard = self.service.storage_key_locks.lock(&storage_key).await;
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
        let staging_key = StorageKey::new(format!(
            "{RESERVED_BLOB_STORAGE_PREFIX}/uploads/generated-{}",
            uuid::Uuid::now_v7()
        ))?;
        let _staging = self.service.blob_storage.create_staged(&staging_key).await?;
        let expected_size = data.len() as u64;
        let staged = self
            .service
            .blob_storage
            .append_staged(
                &staging_key,
                0,
                Box::pin(futures_util::stream::once(async move { Ok(data) })),
            )
            .await?;
        if staged.bytes_written() != expected_size {
            let _ = self.service.blob_storage.discard_staged(&staged).await;
            return Err(CoreError::conflict("generated content size changed"));
        }
        self.service
            .blob_storage
            .publish_staged_if_absent(&staged, &storage_key)
            .await?;
        if let Err(error) = self.service.store.insert(&resource).await {
            let _ = self.service.blob_storage.delete(&storage_key).await;
            let _ = self.service.blob_storage.discard_staged(&staged).await;
            return Err(error);
        }
        self.service.blob_storage.discard_staged(&staged).await?;
        crate::port::LocatedResource::new(resource, directory.clone())
    }

    pub(crate) async fn create(
        &self,
        owner_id: UserId,
        command: CreateUpload,
    ) -> Result<UploadSession, CoreError> {
        let CreateUpload {
            name,
            kind,
            directory_id,
            mime_type,
            expected_size,
            expected_checksum,
        } = command;
        let directory = self.service.directories.locate_by_id(&directory_id).await?;
        let storage_key = StorageKey::from_resource_path(directory.path(), &name)?;
        reject_reserved_storage_key(&storage_key)?;
        let kind = self.service.resolve_content_kind(
            kind,
            mime_type.as_deref(),
            Some(storage_key.as_str()),
        )?;
        build_resource(name.clone(), directory.id(), Some(kind.clone())).build()?;
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

        let session = UploadSession::new(
            owner_id,
            name,
            directory.id(),
            kind,
            mime_type,
            expected_size,
            expected_checksum,
        )?;
        let staged = staged_for(session.id())?;
        self.service
            .blob_storage
            .create_staged(staged.key())
            .await?;
        if let Err(error) = self.service.upload_sessions.save(&session).await {
            let _ = self.service.blob_storage.discard_staged(&staged).await;
            return Err(error);
        }
        Ok(session)
    }

    pub(crate) async fn status(
        &self,
        owner_id: UserId,
        id: &UploadId,
    ) -> Result<UploadSession, CoreError> {
        let _guard = self.service.upload_locks.lock(id).await;
        let session = self.load(owner_id, id).await?;
        if session.status() != UploadStatus::Uploading {
            return Ok(session);
        }
        self.sync_offset(session).await
    }

    pub(crate) async fn append(
        &self,
        owner_id: UserId,
        id: &UploadId,
        requested_offset: u64,
        expected_chunk_checksum: Checksum,
        data: BlobByteStream,
    ) -> Result<UploadSession, CoreError> {
        let _guard = self.service.upload_locks.lock(id).await;
        let session = self.load(owner_id, id).await?;
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
        self.service.blob_storage.discard_staged(&chunk_key).await?;
        let chunk = self
            .service
            .blob_storage
            .create_staged(chunk_key.key())
            .await?;

        let append_result = async {
            let (tracked_data, checksum_state) =
                stream_with_checksum_tracking(limit_stream(data, remaining));
            self.service
                .blob_storage
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
                .blob_storage
                .get_stream(chunk.key())
                .await?
                .ok_or_else(|| CoreError::not_found("staged upload chunk", id.to_string()))?;
            let staged = self
                .service
                .blob_storage
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

        let cleanup_result = self.service.blob_storage.discard_staged(&chunk).await;
        let offset = append_result?;
        cleanup_result?;
        session.synchronize_offset(offset)?;
        Ok(session)
    }

    pub(crate) async fn request_finalization(
        &self,
        owner_id: UserId,
        id: &UploadId,
    ) -> Result<(UploadSession, bool), CoreError> {
        let _upload_guard = self.service.upload_locks.lock(id).await;
        let session = self.load(owner_id, id).await?;
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
            .blob_storage
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

    pub async fn finalize(&self, id: &UploadId) -> Result<Resource, CoreError> {
        let _upload_guard = self.service.upload_locks.lock(id).await;
        let mut session = self.load_unchecked(id).await?;
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
        let id = session.id();
        if let Some(resource) = self
            .service
            .store
            .load(&session.resource_id())
            .await?
        {
            self.service.upload_sessions.mark_completed(&id).await?;
            let _ = self
                .service
                .blob_storage
                .discard_staged(&staged_for(id)?)
                .await;
            return Ok(resource);
        }

        let staged = staged_for(id)?;
        let checksum = match session.actual_checksum() {
            Some(checksum) => checksum.clone(),
            None => {
                let checksum_stream = self
                    .service
                    .blob_storage
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
            .locate_by_id(&session.directory_id())
            .await?;
        let mut resource = build_resource(
            session.name().to_string(),
            directory.id(),
            Some(session.kind().clone()),
        )
        .with_id(session.resource_id())
        .build()?;
        let storage_key = StorageKey::from_resource_path(directory.path(), session.name())?;

        let _storage_guard = self.service.storage_key_locks.lock(&storage_key).await;
        if let Some(existing) = self
            .service
            .read_model
            .find_by_directory_and_name(directory.id(), session.name())
            .await?
        {
            if existing.resource().id() == session.resource_id() {
                self.service.upload_sessions.mark_completed(&id).await?;
                let _ = self.service.blob_storage.discard_staged(&staged).await;
                return Ok(existing.into_resource());
            }
            return Err(CoreError::conflict(format!(
                "resource path `{storage_key}` already exists"
            )));
        }

        let published = match self
            .service
            .blob_storage
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
                let _ = self.service.blob_storage.discard_staged(&staged).await;
                Ok(resource)
            }
            Err(error) => {
                if published || session.status() == UploadStatus::Finalizing {
                    let _ = self.service.blob_storage.delete(&storage_key).await;
                }
                Err(error)
            }
        }
    }

    pub(crate) async fn abort(&self, owner_id: UserId, id: &UploadId) -> Result<(), CoreError> {
        let _guard = self.service.upload_locks.lock(id).await;
        let session = self.load(owner_id, id).await?;
        let staged = staged_for(session.id())?;
        let chunk = chunk_for(session.id())?;
        self.service.blob_storage.discard_staged(&chunk).await?;
        self.service.blob_storage.discard_staged(&staged).await?;
        self.service.upload_sessions.remove(id).await
    }

    async fn load(&self, owner_id: UserId, id: &UploadId) -> Result<UploadSession, CoreError> {
        let session = self.load_unchecked(id).await?;
        if session.owner_id() != owner_id {
            return Err(CoreError::not_found("upload", id.to_string()));
        }
        Ok(session)
    }

    async fn load_unchecked(&self, id: &UploadId) -> Result<UploadSession, CoreError> {
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
        let Some(stream) = self.service.blob_storage.get_stream(storage_key).await? else {
            return Ok(false);
        };
        Ok(calculate_stream_checksum(stream).await? == *expected_checksum)
    }

    async fn sync_offset(&self, mut session: UploadSession) -> Result<UploadSession, CoreError> {
        let id = session.id();
        let actual = self
            .service
            .blob_storage
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

pub struct SecuredUploadService<'a> {
    service: &'a UploadService,
    authorization: &'a AuthorizationService,
    context: &'a AccessContext,
}

impl SecuredUploadService<'_> {
    pub async fn create(&self, command: CreateUpload) -> Result<UploadSession, CoreError> {
        let directory = self
            .service
            .service
            .directories
            .locate_by_id(&command.directory_id())
            .await?;
        self.authorization
            .require(self.context, &directory, DirectoryOperation::CreateResource)
            .await?;
        self.service.create(self.context.user_id(), command).await
    }

    pub async fn status(&self, id: &UploadId) -> Result<UploadSession, CoreError> {
        self.service.status(self.context.user_id(), id).await
    }

    pub async fn append(
        &self,
        id: &UploadId,
        offset: u64,
        expected_chunk_checksum: Checksum,
        data: BlobByteStream,
    ) -> Result<UploadSession, CoreError> {
        self.service
            .append(
                self.context.user_id(),
                id,
                offset,
                expected_chunk_checksum,
                data,
            )
            .await
    }

    pub async fn complete(&self, id: &UploadId) -> Result<(UploadSession, bool), CoreError> {
        self.service
            .request_finalization(self.context.user_id(), id)
            .await
    }

    pub async fn abort(&self, id: &UploadId) -> Result<(), CoreError> {
        self.service.abort(self.context.user_id(), id).await
    }
}

fn staged_for(id: UploadId) -> Result<StagedBlob, CoreError> {
    Ok(StagedBlob::new(
        StorageKey::new(format!("{RESERVED_BLOB_STORAGE_PREFIX}/uploads/{id}"))?,
        0,
    ))
}

fn chunk_for(id: UploadId) -> Result<StagedBlob, CoreError> {
    Ok(StagedBlob::new(
        StorageKey::new(format!("{RESERVED_BLOB_STORAGE_PREFIX}/uploads/{id}.chunk"))?,
        0,
    ))
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
        Ok(chunk)
    }))
}
