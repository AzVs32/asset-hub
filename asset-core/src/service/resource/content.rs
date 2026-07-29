//! 资源内容的上传与读取。
//!
//! 本模块只处理资源内容引用与 Blob 之间的编排；后台扫描协调位于 `reconciliation`。

use super::{ResourceContentStream, ResourceService};
use crate::CoreError;
#[cfg(test)]
use crate::domain::ResourceId;
use crate::domain::{Checksum, ChecksumKind, ResourceContent};
use crate::port::{BlobByteStream, LocatedResource};
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

    #[cfg(test)]
    pub(crate) async fn get_resource_content(
        &self,
        id: &ResourceId,
    ) -> Result<Option<Bytes>, CoreError> {
        let Some(resource) = self.service.query.find_located_by_id(id).await? else {
            return Ok(None);
        };
        self.get_resource_content_snapshot(&resource).await
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

fn hex_digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}
