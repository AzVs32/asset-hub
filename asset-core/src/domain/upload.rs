use super::{Checksum, DirectoryPath, ResourceId, ResourceKind, UserId};
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
    tags: Vec<String>,
    mime_type: Option<String>,
    expected_size: u64,
    offset: u64,
    status: UploadStatus,
    checksum: Option<Checksum>,
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
    pub tags: Vec<String>,
    pub mime_type: Option<String>,
    pub expected_size: u64,
    pub offset: u64,
    pub status: UploadStatus,
    pub checksum: Option<Checksum>,
    pub failure: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl UploadSession {
    pub fn new(
        owner_id: UserId,
        name: impl Into<String>,
        directory: DirectoryPath,
        kind: ResourceKind,
        tags: Vec<String>,
        mime_type: Option<String>,
        expected_size: u64,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: UploadId::new(),
            resource_id: ResourceId::new(),
            owner_id,
            name: name.into(),
            directory,
            kind,
            tags,
            mime_type,
            expected_size,
            offset: 0,
            status: UploadStatus::Uploading,
            checksum: None,
            failure: None,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn rehydrate(snapshot: UploadSessionSnapshot) -> Self {
        Self {
            id: snapshot.id,
            resource_id: snapshot.resource_id,
            owner_id: snapshot.owner_id,
            name: snapshot.name,
            directory: snapshot.directory,
            kind: snapshot.kind,
            tags: snapshot.tags,
            mime_type: snapshot.mime_type,
            expected_size: snapshot.expected_size,
            offset: snapshot.offset,
            status: snapshot.status,
            checksum: snapshot.checksum,
            failure: snapshot.failure,
            created_at: snapshot.created_at,
            updated_at: snapshot.updated_at,
        }
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
    pub fn tags(&self) -> &[String] {
        &self.tags
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
    pub fn checksum(&self) -> Option<&Checksum> {
        self.checksum.as_ref()
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
    pub fn set_offset(&mut self, offset: u64) {
        self.offset = offset;
        self.updated_at = Utc::now();
    }
    pub fn mark_finalizing(&mut self) {
        self.status = UploadStatus::Finalizing;
        self.failure = None;
        self.updated_at = Utc::now();
    }
    pub fn set_checksum(&mut self, checksum: Checksum) {
        self.checksum = Some(checksum);
        self.updated_at = Utc::now();
    }
    pub fn mark_completed(&mut self) {
        self.status = UploadStatus::Completed;
        self.failure = None;
        self.updated_at = Utc::now();
    }
    pub fn mark_failed(&mut self, failure: impl Into<String>) {
        self.status = UploadStatus::Failed;
        self.failure = Some(failure.into());
        self.updated_at = Utc::now();
    }
}
