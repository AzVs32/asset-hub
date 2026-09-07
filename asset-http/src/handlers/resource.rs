//! Resource and Directory aggregate HTTP handlers.

use super::*;

const DEFAULT_PAGE: u32 = 1;
const DEFAULT_LIMIT: u32 = 50;
const MAX_LIMIT: u32 = 100;

/// 分页列出资源。
#[utoipa::path(
    get,
    path = "/resources",
    tag = "resources",
    params(ListResourcesQuery),
    responses(
        (status = 200, description = "资源列表", body = ResourcePageResponse),
        (status = 400, description = "请求参数无效", body = crate::dto::ErrorResponse),
        (status = 500, description = "服务端错误", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn list_resources(
    State(state): State<HttpState>,
    Query(query): Query<ListResourcesQuery>,
) -> Result<Json<ResourcePageResponse>, HttpError> {
    let page = query.page.unwrap_or(DEFAULT_PAGE).max(1);
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset = u64::from(page - 1) * u64::from(limit);
    let directory = query.directory.unwrap_or_default();
    let directory = state.directories().resolve_path(&directory).await?;
    let mut command = ListResources::new(limit, offset, directory.id());

    if let Some(q) = query.q {
        command = command.with_q(q);
    }

    let page_result = state.resources().list(command).await?;

    Ok(Json(resource_page_response(page_result, page)))
}

/// 按 ID 查询资源。
#[utoipa::path(
    get,
    path = "/resources/{id}",
    tag = "resources",
    params(
        ("id" = String, Path, description = "资源 ID")
    ),
    responses(
        (status = 200, description = "资源详情", body = ResourceResponse),
        (status = 400, description = "请求参数无效", body = crate::dto::ErrorResponse),
        (status = 404, description = "资源不存在", body = crate::dto::ErrorResponse),
        (status = 500, description = "服务端错误", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn find_resource(
    State(state): State<HttpState>,
    Path(id): Path<String>,
) -> Result<Json<ResourceResponse>, HttpError> {
    let id = parse_resource_id(&id)?;

    match state.resources().get(&id).await? {
        Some(resource) => Ok(Json(resource_response(&resource))),
        None => Err(HttpError::not_found(format!("resource `{id}` not found"))),
    }
}

/// 更新资源。
#[utoipa::path(
    patch,
    path = "/resources/{id}",
    tag = "resources",
    params(
        ("id" = String, Path, description = "资源 ID")
    ),
    request_body = UpdateResourceRequest,
    responses(
        (status = 200, description = "资源已更新", body = ResourceResponse),
        (status = 400, description = "请求参数无效", body = crate::dto::ErrorResponse),
        (status = 404, description = "资源不存在", body = crate::dto::ErrorResponse),
        (status = 500, description = "服务端错误", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn update_resource(
    State(state): State<HttpState>,
    Path(id): Path<String>,
    payload: Result<Json<UpdateResourceRequest>, JsonRejection>,
) -> Result<Json<ResourceResponse>, HttpError> {
    let id = parse_resource_id(&id)?;
    let payload = parse_json_payload(payload)?;
    let mut command = UpdateResource::new(payload.expected_revision);

    if let Some(name) = payload.name {
        command = command.with_name(name);
    }

    if let Some(directory_id) = payload.directory_id {
        command = command.with_directory_id(parse_directory_id(&directory_id)?);
    }

    match state.resources().update(&id, command).await? {
        Some(resource) => Ok(Json(
            resource_snapshot_response(state.resources(), &resource).await?,
        )),
        None => Err(HttpError::not_found(format!("resource `{id}` not found"))),
    }
}

/// Permanently delete a Resource and its physical content.
#[utoipa::path(
    delete,
    path = "/resources/{id}",
    tag = "resources",
    params(
        ("id" = String, Path, description = "资源 ID"),
        ExpectedRevisionQuery
    ),
    responses(
        (status = 200, description = "资源已删除", body = ResourceResponse),
        (status = 400, description = "请求参数无效", body = crate::dto::ErrorResponse),
        (status = 404, description = "资源不存在", body = crate::dto::ErrorResponse),
        (status = 409, description = "资源版本已变化", body = crate::dto::ErrorResponse),
        (status = 500, description = "服务端错误", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn delete_resource(
    State(state): State<HttpState>,
    Path(id): Path<String>,
    Query(query): Query<ExpectedRevisionQuery>,
) -> Result<StatusCode, HttpError> {
    let id = parse_resource_id(&id)?;

    if state
        .resources()
        .delete(&id, query.expected_revision)
        .await?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(HttpError::not_found(format!("resource `{id}` not found")))
    }
}

pub(super) fn resource_response(resource: &asset_core::port::LocatedResource) -> ResourceResponse {
    ResourceResponse::new(resource.resource(), resource.directory().path().clone())
}

pub(super) async fn resource_snapshot_response(
    service: &asset_core::service::ResourceService,
    resource: &asset_core::domain::Resource,
) -> Result<ResourceResponse, CoreError> {
    let directory = service.locate_resource_directory(resource).await?;
    Ok(ResourceResponse::new(resource, directory.path().clone()))
}

pub(super) fn resource_page_response(
    page_result: asset_core::port::ResourcePage,
    page: u32,
) -> ResourcePageResponse {
    let mut items = Vec::with_capacity(page_result.items.len());
    for resource in &page_result.items {
        items.push(resource_response(resource));
    }
    ResourcePageResponse {
        items,
        total: page_result.total,
        page,
        limit: page_result.limit,
    }
}

pub(super) fn parse_resource_id(value: &str) -> Result<ResourceId, HttpError> {
    ResourceId::from_str(value).map_err(|error| HttpError::bad_request(error.to_string()))
}

pub(super) fn parse_json_payload<T>(
    payload: Result<Json<T>, JsonRejection>,
) -> Result<T, HttpError> {
    payload.map(|Json(payload)| payload).map_err(|error| {
        if error.status() == StatusCode::PAYLOAD_TOO_LARGE {
            HttpError::payload_too_large(error.body_text())
        } else {
            HttpError::bad_request(format!("invalid JSON request body: {error}"))
        }
    })
}
