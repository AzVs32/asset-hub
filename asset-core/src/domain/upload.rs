use super::{Checksum, DirectoryPath, Resource, ResourceContent, ResourceId, ResourceKind, UserId};
use crate::ResourceError;
use chrono::{DateTime, Utc};

crate::gen_id_uuid_v7!(UploadId);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadStatus {
    Uploading,
    Finalizing,
    Completed,
    Failed,
}

impl UploadStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Uploading => "uploading",
            Self::Finalizing => "finalizing",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

/// 尚未发布为 Resource 的持久化上传会话。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadSession {
    id: UploadId,
    resource_id: ResourceId,
    owner_id: UserId,
    name: String,
    directory: DirectoryPath,
    kind: ResourceKind,
    mime_type: Option<String>,
    expected_size: u64,
    offset: u64,
    status: UploadStatus,
    expected_checksum: Checksum,
    actual_checksum: Option<Checksum>,
    failure: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct UploadSessionSnapshot {
    pub id: UploadId,
    pub resource_id: ResourceId,
    pub owner_id: UserId,
    pub name: String,
    pub directory: DirectoryPath,
    pub kind: ResourceKind,
    pub mime_type: Option<String>,
    pub expected_size: u64,
    pub offset: u64,
    pub status: UploadStatus,
    pub expected_checksum: Checksum,
    pub actual_checksum: Option<Checksum>,
    pub failure: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl UploadSession {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        owner_id: UserId,
        name: impl Into<String>,
        directory: DirectoryPath,
        kind: ResourceKind,
        mime_type: Option<String>,
        expected_size: u64,
        expected_checksum: Checksum,
    ) -> Result<Self, ResourceError> {
        let now = Utc::now();
        Self::rehydrate(UploadSessionSnapshot {
            id: UploadId::new(),
            resource_id: ResourceId::new(),
            owner_id,
            name: name.into(),
            directory,
            kind,
            mime_type,
            expected_size,
            offset: 0,
            status: UploadStatus::Uploading,
            expected_checksum,
            actual_checksum: None,
            failure: None,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn rehydrate(snapshot: UploadSessionSnapshot) -> Result<Self, ResourceError> {
        Resource::builder(snapshot.name.clone())
            .with_kind(snapshot.kind.clone())
            .build()?;
        if let Some(mime_type) = &snapshot.mime_type {
            ResourceContent::pending(snapshot.expected_size)
                .with_mime_type(mime_type.clone())
                .build()?;
        }
        validate_upload_state(&snapshot)?;

        Ok(Self {
            id: snapshot.id,
            resource_id: snapshot.resource_id,
            owner_id: snapshot.owner_id,
            name: snapshot.name,
            directory: snapshot.directory,
            kind: snapshot.kind,
            mime_type: snapshot.mime_type,
            expected_size: snapshot.expected_size,
            offset: snapshot.offset,
            status: snapshot.status,
            expected_checksum: snapshot.expected_checksum,
            actual_checksum: snapshot.actual_checksum,
            failure: snapshot.failure,
            created_at: snapshot.created_at,
            updated_at: snapshot.updated_at,
        })
    }

    pub fn id(&self) -> UploadId {
        self.id
    }
    pub fn resource_id(&self) -> ResourceId {
        self.resource_id
    }
    pub fn owner_id(&self) -> UserId {
        self.owner_id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn directory(&self) -> &DirectoryPath {
        &self.directory
    }
    pub fn kind(&self) -> &ResourceKind {
        &self.kind
    }
    pub fn mime_type(&self) -> Option<&str> {
        self.mime_type.as_deref()
    }
    pub fn expected_size(&self) -> u64 {
        self.expected_size
    }
    pub fn offset(&self) -> u64 {
        self.offset
    }
    pub fn status(&self) -> UploadStatus {
        self.status
    }
    pub fn expected_checksum(&self) -> &Checksum {
        &self.expected_checksum
    }
    pub fn actual_checksum(&self) -> Option<&Checksum> {
        self.actual_checksum.as_ref()
    }
    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    pub fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }
    pub fn synchronize_offset(&mut self, offset: u64) -> Result<(), ResourceError> {
        if self.status != UploadStatus::Uploading {
            return Err(invalid_upload_state(
                "only an uploading session can change its offset",
            ));
        }
        if offset > self.expected_size {
            return Err(invalid_upload_state(
                "upload offset cannot exceed its expected size",
            ));
        }
        self.offset = offset;
        self.updated_at = Utc::now();
        Ok(())
    }
    pub fn mark_finalizing(&mut self) -> Result<(), ResourceError> {
        if !matches!(self.status, UploadStatus::Uploading | UploadStatus::Failed) {
            return Err(invalid_upload_state(
                "only an uploading or failed session can begin finalization",
            ));
        }
        if self.offset != self.expected_size {
            return Err(invalid_upload_state(
                "upload must be complete before finalization",
            ));
        }
        self.status = UploadStatus::Finalizing;
        self.failure = None;
        self.updated_at = Utc::now();
        Ok(())
    }
    pub fn set_actual_checksum(&mut self, checksum: Checksum) -> Result<(), ResourceError> {
        if self.status != UploadStatus::Finalizing {
            return Err(invalid_upload_state(
                "actual checksum can only be recorded during finalization",
            ));
        }
        self.actual_checksum = Some(checksum);
        self.updated_at = Utc::now();
        Ok(())
    }
    pub fn mark_completed(&mut self) -> Result<(), ResourceError> {
        if self.status != UploadStatus::Finalizing
            || self.actual_checksum.as_ref() != Some(&self.expected_checksum)
        {
            return Err(invalid_upload_state(
                "only a finalizing session with the expected checksum can be completed",
            ));
        }
        self.status = UploadStatus::Completed;
        self.failure = None;
        self.updated_at = Utc::now();
        Ok(())
    }
    pub fn mark_failed(&mut self, failure: impl Into<String>) -> Result<(), ResourceError> {
        if self.status != UploadStatus::Finalizing {
            return Err(invalid_upload_state(
                "only a finalizing session can fail finalization",
            ));
        }
        let failure = failure.into();
        if failure.trim().is_empty() {
            return Err(ResourceError::Blank {
                field: "upload.failure",
            });
        }
        self.status = UploadStatus::Failed;
        self.failure = Some(failure);
        self.updated_at = Utc::now();
        Ok(())
    }
}

fn validate_upload_state(snapshot: &UploadSessionSnapshot) -> Result<(), ResourceError> {
    if snapshot.updated_at < snapshot.created_at {
        return Err(invalid_upload_state(
            "updated timestamp cannot precede creation",
        ));
    }
    if snapshot.offset > snapshot.expected_size {
        return Err(invalid_upload_state(
            "upload offset cannot exceed its expected size",
        ));
    }

    match snapshot.status {
        UploadStatus::Uploading => {
            if snapshot.actual_checksum.is_some() || snapshot.failure.is_some() {
                return Err(invalid_upload_state(
                    "uploading session cannot have a checksum or failure",
                ));
            }
        }
        UploadStatus::Finalizing => {
            require_complete_offset(snapshot)?;
            if snapshot.failure.is_some() {
                return Err(invalid_upload_state(
                    "finalizing session cannot have a failure",
                ));
            }
        }
        UploadStatus::Completed => {
            require_complete_offset(snapshot)?;
            if snapshot.actual_checksum.as_ref() != Some(&snapshot.expected_checksum)
                || snapshot.failure.is_some()
            {
                return Err(invalid_upload_state(
                    "completed session requires the expected checksum and no failure",
                ));
            }
        }
        UploadStatus::Failed => {
            require_complete_offset(snapshot)?;
            if snapshot
                .failure
                .as_deref()
                .is_none_or(|failure| failure.trim().is_empty())
            {
                return Err(invalid_upload_state(
                    "failed session requires a failure reason",
                ));
            }
        }
    }
    Ok(())
}

fn require_complete_offset(snapshot: &UploadSessionSnapshot) -> Result<(), ResourceError> {
    if snapshot.offset != snapshot.expected_size {
        return Err(invalid_upload_state(
            "finalization requires the expected number of bytes",
        ));
    }
    Ok(())
}

fn invalid_upload_state(reason: &'static str) -> ResourceError {
    ResourceError::InvalidFormat {
        field: "upload.status",
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checksum(value: char) -> Checksum {
        Checksum::sha256(value.to_string().repeat(64)).unwrap()
    }

    fn session(expected_size: u64) -> UploadSession {
        UploadSession::new(
            UserId::new(),
            "asset.bin",
            DirectoryPath::root(),
            ResourceKind::default(),
            None,
            expected_size,
            checksum('a'),
        )
        .unwrap()
    }

    #[test]
    fn upload_state_machine_accepts_the_valid_finalization_path() {
        let mut session = session(4);
        session.synchronize_offset(4).unwrap();
        session.mark_finalizing().unwrap();
        session.set_actual_checksum(checksum('a')).unwrap();
        session.mark_completed().unwrap();

        assert_eq!(session.status(), UploadStatus::Completed);
    }

    #[test]
    fn upload_state_machine_rejects_incomplete_or_out_of_order_transitions() {
        let mut session = session(4);
        assert!(session.mark_finalizing().is_err());
        assert!(session.synchronize_offset(5).is_err());
        assert!(session.set_actual_checksum(checksum('a')).is_err());
        assert!(session.mark_failed("failure").is_err());
    }

    #[test]
    fn upload_rehydration_rejects_inconsistent_terminal_state() {
        let session = session(4);
        let now = Utc::now();
        let snapshot = UploadSessionSnapshot {
            id: session.id(),
            resource_id: session.resource_id(),
            owner_id: session.owner_id(),
            name: session.name().to_string(),
            directory: session.directory().clone(),
            kind: session.kind().clone(),
            mime_type: None,
            expected_size: 4,
            offset: 4,
            status: UploadStatus::Completed,
            expected_checksum: checksum('a'),
            actual_checksum: None,
            failure: None,
            created_at: now,
            updated_at: now,
        };

        assert!(UploadSession::rehydrate(snapshot).is_err());
    }
}
