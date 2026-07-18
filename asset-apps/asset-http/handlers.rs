use crate::dto::{
    AuditStorageIssueResponse, AuditStorageRequest, AuditStorageResponse, BinaryContent,
    CreateDirectoryRequest, CreateResourceRequest, DirectoryListingResponse,
    ExecuteResourceActionRequest, HealthResponse, ListDirectoryQuery, ListResourcesQuery,
    ResourceActionOutputResponse, ResourceDirectoryResponse, ResourceKindResponse,
    ResourceKindsResponse, ResourcePageResponse, ResourceReadResponse, ResourceResponse,
    ScanStorageErrorResponse, ScanStorageRequest, ScanStorageResponse, UpdateResourceRequest,
    UploadResourceContentStreamQuery,
};
use crate::error::HttpError;
use crate::state::HttpState;
use asset_core::CoreError;
use asset_core::domain::{
    AccessContext, ResourceDirectory, ResourceId, ResourceKind, ResourceStatus,
};
use asset_core::port::BlobByteStream;
use asset_core::port::ListResources;
use asset_core::service::{
    AuditStorage, CreateResource, ExecuteResourceAction, ScanStorage, UpdateResource,
    UploadResourceContentStream,
};
use axum::Json;
use axum::body::Body;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use futures_util::StreamExt;
use std::path::{Component, Path as FsPath};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) mod content;
pub(crate) mod maintenance;
pub(crate) mod plugin;
pub(crate) mod resource;

pub(crate) use content::{
    MAX_UPLOAD_BYTES, get_resource_content, preview_resource, read_resource, thumbnail_resource,
    upload_resource_content_stream,
};
pub(crate) use maintenance::{audit_storage, health, purge_disabled, scan_storage};
pub(crate) use plugin::plugin_web_asset;
pub(crate) use resource::{
    MAX_ACTION_REQUEST_BYTES, create_directory, create_resource, execute_resource_action,
    find_resource, list_directory, list_resource_kinds, list_resources, remove_resource,
    soft_delete_resource, update_resource,
};

use content::*;
use resource::*;
