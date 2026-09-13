//! Public application contracts for the independently assembled Resource-related services.

use crate::{
    directory::domain::DirectoryId, idempotency::domain::IdempotencyKey,
    resource::domain::Checksum, storage::port::BlobByteStream,
};

/// 单次追加上传允许的最大分片长度：8 MiB。
///
/// 该值是上传协议的一部分，用于限制分片临时文件和单次请求的资源占用，不限制资源总大小。
pub const MAX_UPLOAD_CHUNK_SIZE: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct CreateUpload {
    pub(super) name: String,
    pub(super) directory_id: DirectoryId,
    pub(super) mime_type: Option<String>,
    pub(super) expected_size: u64,
    pub(super) expected_checksum: Checksum,
    pub(super) idempotency_key: Option<IdempotencyKey>,
}

impl CreateUpload {
    pub fn new(
        name: impl Into<String>,
        directory_id: DirectoryId,
        expected_size: u64,
        expected_checksum: Checksum,
    ) -> Self {
        Self {
            name: name.into(),
            directory_id,
            mime_type: None,
            expected_size,
            expected_checksum,
            idempotency_key: None,
        }
    }

    pub fn with_mime_type(mut self, mime_type: impl Into<String>) -> Self {
        self.mime_type = Some(mime_type.into());
        self
    }

    pub fn with_idempotency_key(mut self, key: IdempotencyKey) -> Self {
        self.idempotency_key = Some(key);
        self
    }

    pub fn directory_id(&self) -> DirectoryId {
        self.directory_id
    }

    pub fn idempotency_key(&self) -> Option<&IdempotencyKey> {
        self.idempotency_key.as_ref()
    }
}

#[derive(Debug, Clone)]
pub struct CreateContentReplacementUpload {
    pub(super) expected_size: u64,
    pub(super) expected_checksum: Checksum,
    pub(super) expected_revision: u64,
    pub(super) mime_type: Option<String>,
    pub(super) idempotency_key: Option<IdempotencyKey>,
}

impl CreateContentReplacementUpload {
    pub fn new(expected_size: u64, expected_checksum: Checksum, expected_revision: u64) -> Self {
        Self {
            expected_size,
            expected_checksum,
            expected_revision,
            mime_type: None,
            idempotency_key: None,
        }
    }

    pub fn with_mime_type(mut self, mime_type: impl Into<String>) -> Self {
        self.mime_type = Some(mime_type.into());
        self
    }

    pub fn with_idempotency_key(mut self, key: IdempotencyKey) -> Self {
        self.idempotency_key = Some(key);
        self
    }

    pub fn idempotency_key(&self) -> Option<&IdempotencyKey> {
        self.idempotency_key.as_ref()
    }
}

#[derive(Debug, Clone, Default)]
pub struct UpdateResource {
    pub(super) expected_revision: u64,
    pub(super) name: Option<String>,
    pub(super) directory_id: Option<DirectoryId>,
}

impl UpdateResource {
    pub fn new(expected_revision: u64) -> Self {
        Self {
            expected_revision,
            ..Self::default()
        }
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn with_directory_id(mut self, directory_id: DirectoryId) -> Self {
        self.directory_id = Some(directory_id);
        self
    }

    pub fn directory_id(&self) -> Option<DirectoryId> {
        self.directory_id
    }
}

pub struct ResourceContentStream {
    content_type: String,
    content_length: u64,
    content: BlobByteStream,
}

impl ResourceContentStream {
    pub(super) fn new(content_type: String, content_length: u64, content: BlobByteStream) -> Self {
        Self {
            content_type,
            content_length,
            content,
        }
    }

    pub fn content_type(&self) -> &str {
        &self.content_type
    }

    pub fn content_length(&self) -> u64 {
        self.content_length
    }

    pub fn into_content(self) -> BlobByteStream {
        self.content
    }
}
