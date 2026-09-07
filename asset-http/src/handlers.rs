use crate::dto::{
    BinaryContent, CreateDirectoryRequest, CreateUploadRequest, DirectoryListingResponse,
    DirectoryResponse, ExpectedRevisionQuery, HealthResponse, ListDirectoryQuery,
    ListResourcesQuery, ResourcePageResponse, ResourceResponse, UpdateDirectoryRequest,
    UpdateResourceRequest, UploadSessionResponse,
};
use crate::error::HttpError;
use crate::state::HttpState;
use asset_core::CoreError;
use asset_core::domain::{
    Checksum, DirectoryId, IdempotencyKey, ResourceId, UploadId, UploadSession,
};
use asset_core::port::BlobByteStream;
use asset_core::port::ListResources;
use asset_core::service::{CreateUpload, UpdateDirectory, UpdateResource};
use axum::Json;
use axum::body::Body;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures_util::StreamExt;
use std::str::FromStr;

pub(crate) mod content;
pub(crate) mod directory;
pub(crate) mod maintenance;
pub(crate) mod resource;
pub(crate) mod upload;

pub(crate) use content::{
    download_directory, download_resource_content, get_resource_content, replace_resource_content,
};
pub(crate) use directory::{
    create_directory, delete_directory, find_directory, list_directory, update_directory,
};
pub(crate) use maintenance::health;
pub(crate) use resource::{delete_resource, find_resource, list_resources, update_resource};
pub(crate) use upload::{
    abort_upload, append_upload, complete_upload, create_upload, upload_status,
};

use directory::*;
use resource::*;

const IDEMPOTENCY_KEY: header::HeaderName = header::HeaderName::from_static("idempotency-key");

pub(super) fn parse_idempotency_key(
    headers: &HeaderMap,
) -> Result<Option<IdempotencyKey>, HttpError> {
    let Some(value) = headers.get(IDEMPOTENCY_KEY) else {
        return Ok(None);
    };
    let value = value
        .to_str()
        .map_err(|_| HttpError::bad_request("invalid idempotency-key header"))?;
    IdempotencyKey::new(value)
        .map(Some)
        .map_err(|error| HttpError::bad_request(error.to_string()))
}
