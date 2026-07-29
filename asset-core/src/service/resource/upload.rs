use super::command::build_resource;
use super::content::{build_verified_content, calculate_stream_checksum};
use super::{CreateUpload, ResourceService};
use crate::CoreError;
use crate::domain::{
    Checksum, Resource, StorageKey, UploadId, UploadSession, UploadStatus, UserId,
};
use crate::port::{BlobByteStream, RESERVED_BLOB_STORAGE_PREFIX, StagedBlob};
use futures_util::StreamExt;

pub(super) struct ResourceUploadService<'a> {
    service: &'a ResourceService,
}

impl<'a> ResourceUploadService<'a> {
    pub(super) fn new(service: &'a ResourceService) -> Self {
        Self { service }
    }

    pub(crate) async fn create(
        &self,
        owner_id: UserId,
        command: CreateUpload,
    ) -> Result<UploadSession, CoreError> {
        let CreateUpload {
            name,
            kind,
            directory,
            tags,
            mime_type,
            expected_size,
        } = command;
        let storage_key = StorageKey::from_resource_path(&directory, &name)?;
        reject_reserved_storage_key(&storage_key)?;
        let kind = self.service.resolve_content_kind(
            kind,
            mime_type.as_deref(),
            Some(storage_key.as_str()),
        )?;
        let directory = self.service.directories.ensure_path(&directory).await?;
        build_resource(
            name.clone(),
            directory.id(),
            Some(kind.clone()),
            tags.clone(),
        )
        .build()?;
        if self
            .service
            .query
            .find_by_path(directory.path(), &name)
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
            directory.path().clone(),
            kind,
            tags,
            mime_type,
            expected_size,
        );
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
        let staged = self
            .service
            .blob_storage
            .append_staged(
                staged_key.key(),
                session.offset(),
                limit_stream(data, remaining),
            )
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
        session.set_offset(staged.bytes_written());
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
        if !self.service.upload_sessions.mark_finalizing(id).await? {
            return Err(CoreError::conflict(
                "upload session status changed concurrently",
            ));
        }
        let mut session = session;
        session.mark_finalizing();
        Ok((session, true))
    }

    pub(crate) async fn finalize(&self, id: &UploadId) -> Result<Resource, CoreError> {
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
            .repository
            .find_by_id(&session.resource_id())
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
        let checksum = match session.checksum() {
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
                    .save_checksum(&id, &checksum)
                    .await?;
                session.set_checksum(checksum.clone());
                checksum
            }
        };
        let directory = self
            .service
            .directories
            .resolve_path(session.directory())
            .await?;
        let mut resource = build_resource(
            session.name().to_string(),
            directory.id(),
            Some(session.kind().clone()),
            session.tags().to_vec(),
        )
        .with_id(session.resource_id())
        .build()?;
        let storage_key = StorageKey::from_resource_path(directory.path(), session.name())?;

        let _storage_guard = self.service.storage_key_locks.lock(&storage_key).await;
        if let Some(existing) = self
            .service
            .query
            .find_by_path(directory.path(), session.name())
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
            self.service.repository.save(&resource).await?;
            if let Err(error) = self.service.upload_sessions.mark_completed(&id).await {
                let _ = self.service.repository.remove(&resource.id()).await;
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
            .repository
            .find_by_id(&session.resource_id())
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
            session.set_offset(actual);
        }
        Ok(session)
    }
}

fn staged_for(id: UploadId) -> Result<StagedBlob, CoreError> {
    Ok(StagedBlob::new(
        StorageKey::new(format!("{RESERVED_BLOB_STORAGE_PREFIX}/uploads/{id}"))?,
        0,
    ))
}

fn reject_reserved_storage_key(key: &StorageKey) -> Result<(), CoreError> {
    if key.as_str() == RESERVED_BLOB_STORAGE_PREFIX
        || key
            .as_str()
            .starts_with(&format!("{RESERVED_BLOB_STORAGE_PREFIX}/"))
    {
        return Err(CoreError::configuration(format!(
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
            .ok_or_else(|| CoreError::configuration("upload size overflow"))?;
        if received > remaining {
            return Err(CoreError::conflict(
                "upload chunk exceeds the declared upload size",
            ));
        }
        Ok(chunk)
    }))
}
