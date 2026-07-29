use crate::dto::{
    BinaryContent, CreateDirectoryRequest, CreateUploadRequest, DirectoryActionOutputResponse,
    DirectoryKindResponse, DirectoryKindsResponse, DirectoryListingResponse, DirectoryResponse,
    ExecuteDirectoryActionRequest, ExecuteResourceActionRequest, HealthResponse,
    ListDirectoryQuery, ListResourcesQuery, ResourceActionOutputResponse, ResourceKindResponse,
    ResourceKindsResponse, ResourcePageResponse, ResourceResponse, UpdateResourceRequest,
    UploadSessionResponse,
};
use crate::error::HttpError;
use crate::state::HttpState;
use asset_core::CoreError;
use asset_core::domain::{
    AccessContext, DirectoryId, DirectoryKind, ResourceId, ResourceKind, UploadId, UploadSession,
};
use asset_core::port::BlobByteStream;
use asset_core::port::ListResources;
use asset_core::service::{
    CreateUpload, ExecuteDirectoryAction, ExecuteResourceAction, UpdateResource,
};
use axum::Json;
use axum::body::Body;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures_util::StreamExt;
use std::path::{Component, Path as FsPath};
use std::str::FromStr;

pub(crate) mod content;
pub(crate) mod maintenance;
pub(crate) mod plugin;
pub(crate) mod resource;
pub(crate) mod upload;

pub(crate) use content::{download_directory, download_resource_content, get_resource_content};
pub(crate) use maintenance::{health, purge_disabled};
pub(crate) use plugin::plugin_web_asset;
pub(crate) use resource::{
    MAX_ACTION_REQUEST_BYTES, create_directory, execute_directory_action, execute_resource_action,
    find_resource, list_directory, list_directory_kinds, list_resource_kinds, list_resources,
    remove_resource, soft_delete_resource, update_resource,
};
pub(crate) use upload::{
    abort_upload, append_upload, complete_upload, create_upload, upload_status,
};

use content::*;
use resource::*;
