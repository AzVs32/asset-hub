use super::*;

pub(crate) const MAX_ACTION_REQUEST_BYTES: usize = 1024 * 1024;
const DEFAULT_PAGE: u32 = 1;
const DEFAULT_LIMIT: u32 = 50;
const MAX_LIMIT: u32 = 100;

/// 列出当前后端支持的资源类型。
#[utoipa::path(
    get,
    path = "/resource-kinds",
    tag = "resources",
    responses(
        (status = 200, description = "资源类型列表", body = ResourceKindsResponse)
    )
)]
pub(crate) async fn list_resource_kinds(
    State(state): State<HttpState>,
) -> Json<ResourceKindsResponse> {
    Json(ResourceKindsResponse {
        items: state
            .kind_registry()
            .definitions()
            .iter()
            .map(|definition| {
                ResourceKindResponse::from_definition(
                    definition,
                    state.kind_registry(),
                    state.service(),
                )
            })
            .collect(),
    })
}

/// 创建不包含对象内容的资源。
#[utoipa::path(
    post,
    path = "/resources",
    tag = "resources",
    request_body = CreateResourceRequest,
    responses(
        (status = 201, description = "资源已创建", body = ResourceResponse),
        (status = 400, description = "请求参数无效", body = crate::dto::ErrorResponse),
        (status = 500, description = "服务端错误", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn create_resource(
    State(state): State<HttpState>,
    access: Extension<AccessContext>,
    payload: Result<Json<CreateResourceRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<ResourceResponse>), HttpError> {
    let payload = parse_json_payload(payload)?;
    let command = apply_common_resource_fields(
        CreateResource::new(payload.name),
        payload.kind,
        payload.status,
        payload.directory,
        payload.description,
        payload.tags,
    )?;
    let resource = state.secured(&access.0).create_resource(command).await?;

    Ok((
        StatusCode::CREATED,
        Json(resource_response(state.service(), &resource)?),
    ))
}

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
    access: Extension<AccessContext>,
    Query(query): Query<ListResourcesQuery>,
) -> Result<Json<ResourcePageResponse>, HttpError> {
    let page = query.page.unwrap_or(DEFAULT_PAGE).max(1);
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset = u64::from(page - 1) * u64::from(limit);
    let mut command = ListResources::new(limit, offset)
        .with_include_deleted(query.include_deleted.unwrap_or(false));

    if let Some(kind) = query.kind {
        command = command.with_kind(parse_kind(kind)?);
    }
    command = command.with_include_descendants(query.include_descendants.unwrap_or(false));

    if let Some(tag) = query.tag {
        command = command.with_tag(tag);
    }

    if let Some(q) = query.q {
        command = command.with_q(q);
    }

    if let Some(directory) = query.directory {
        command = command.with_directory(directory);
    }

    let page_result = state.secured(&access.0).list_resources(command).await?;

    Ok(Json(resource_page_response(
        state.service(),
        page_result,
        page,
    )?))
}

/// 列出当前目录的直接子目录和资源。
#[utoipa::path(
    get,
    path = "/directories",
    tag = "resources",
    params(ListDirectoryQuery),
    responses(
        (status = 200, description = "目录列表", body = DirectoryListingResponse),
        (status = 400, description = "请求参数无效", body = crate::dto::ErrorResponse),
        (status = 500, description = "服务端错误", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn list_directory(
    State(state): State<HttpState>,
    access: Extension<AccessContext>,
    Query(query): Query<ListDirectoryQuery>,
) -> Result<Json<DirectoryListingResponse>, HttpError> {
    let directory = query.path.unwrap_or_default();
    let page = query.page.unwrap_or(DEFAULT_PAGE).max(1);
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset = u64::from(page - 1) * u64::from(limit);
    let mut resources_query = ListResources::new(limit, offset)
        .with_directory(directory.clone())
        .with_include_deleted(query.include_deleted.unwrap_or(false));

    if let Some(kind) = query.kind {
        resources_query = resources_query.with_kind(parse_kind(kind)?);
    }
    resources_query =
        resources_query.with_include_descendants(query.include_descendants.unwrap_or(false));

    if let Some(tag) = query.tag {
        resources_query = resources_query.with_tag(tag);
    }

    if let Some(q) = query.q {
        resources_query = resources_query.with_q(q);
    }

    let folders = state
        .secured(&access.0)
        .list_directories(&directory)
        .await?
        .into_iter()
        .map(|directory| ResourceDirectoryResponse {
            path: directory.path().to_owned(),
            parent_path: directory.parent_path().to_owned(),
            name: directory.name().to_owned(),
        })
        .collect();
    let resources = state
        .secured(&access.0)
        .list_resources(resources_query)
        .await?;

    Ok(Json(DirectoryListingResponse {
        path: directory,
        folders,
        resources: resource_page_response(state.service(), resources, page)?,
    }))
}

/// 创建一个空逻辑目录。
#[utoipa::path(
    post,
    path = "/directories",
    tag = "resources",
    request_body = CreateDirectoryRequest,
    responses(
        (status = 201, description = "目录已创建", body = ResourceDirectoryResponse),
        (status = 400, description = "目录名称无效", body = crate::dto::ErrorResponse),
        (status = 403, description = "没有父目录写权限", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn create_directory(
    State(state): State<HttpState>,
    access: Extension<AccessContext>,
    payload: Result<Json<CreateDirectoryRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<ResourceDirectoryResponse>), HttpError> {
    let payload = parse_json_payload(payload)?;
    let directory = state
        .secured(&access.0)
        .create_directory(&payload.parent_path, payload.name)
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(ResourceDirectoryResponse {
            path: directory.path().to_owned(),
            parent_path: directory.parent_path().to_owned(),
            name: directory.name().to_owned(),
        }),
    ))
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
    access: Extension<AccessContext>,
    Path(id): Path<String>,
) -> Result<Json<ResourceResponse>, HttpError> {
    let id = parse_resource_id(&id)?;

    match state.secured(&access.0).find_resource(&id).await? {
        Some(resource) => Ok(Json(resource_response(state.service(), &resource)?)),
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
    access: Extension<AccessContext>,
    Path(id): Path<String>,
    payload: Result<Json<UpdateResourceRequest>, JsonRejection>,
) -> Result<Json<ResourceResponse>, HttpError> {
    let id = parse_resource_id(&id)?;
    let payload = parse_json_payload(payload)?;
    let mut command = UpdateResource::new();

    if let Some(name) = payload.name {
        command = command.with_name(name);
    }

    if let Some(kind) = payload.kind {
        command = command.with_kind(parse_kind(kind)?);
    }

    if let Some(status) = payload.status {
        command = command.with_status(parse_status(&status)?);
    }

    if let Some(directory) = payload.directory {
        command = command.with_directory(directory);
    }

    if let Some(description) = payload.description {
        command = command.with_description(description);
    }

    if let Some(tags) = payload.tags {
        command = command.with_tags(tags);
    }

    if let Some(restore) = payload.restore {
        command = command.with_restore(restore);
    }

    match state
        .secured(&access.0)
        .update_resource(&id, command)
        .await?
    {
        Some(resource) => Ok(Json(resource_response(state.service(), &resource)?)),
        None => Err(HttpError::not_found(format!("resource `{id}` not found"))),
    }
}

/// 执行资源插件动作。
#[utoipa::path(
    post,
    path = "/resources/{id}/actions/{action}",
    tag = "resources",
    request_body = ExecuteResourceActionRequest,
    params(
        ("id" = String, Path, description = "资源 ID"),
        ("action" = String, Path, description = "动作 ID")
    ),
    responses(
        (status = 200, description = "动作执行结果", body = ResourceActionOutputResponse),
        (status = 400, description = "资源类型不支持该动作或动作未配置", body = crate::dto::ErrorResponse),
        (status = 404, description = "资源不存在", body = crate::dto::ErrorResponse),
        (status = 500, description = "插件或服务端错误", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn execute_resource_action(
    State(state): State<HttpState>,
    access: Extension<AccessContext>,
    Path((id, action)): Path<(String, String)>,
    payload: Result<Json<ExecuteResourceActionRequest>, JsonRejection>,
) -> Result<Json<ResourceActionOutputResponse>, HttpError> {
    let id = parse_resource_id(&id)?;
    let payload = payload.map_err(|error| {
        if error.status() == StatusCode::PAYLOAD_TOO_LARGE {
            HttpError::payload_too_large(error.body_text())
        } else {
            HttpError::bad_request(error.body_text())
        }
    })?;
    let command = ExecuteResourceAction::new(action).with_input(payload.input.clone());
    let Some(output) = state
        .secured(&access.0)
        .execute_resource_action(&id, command)
        .await?
    else {
        return Err(HttpError::not_found(format!("resource `{id}` not found")));
    };

    Ok(Json(ResourceActionOutputResponse::from(&output)))
}

/// 软删除资源。
#[utoipa::path(
    delete,
    path = "/resources/{id}",
    tag = "resources",
    params(
        ("id" = String, Path, description = "资源 ID")
    ),
    responses(
        (status = 200, description = "资源已软删除", body = ResourceResponse),
        (status = 400, description = "请求参数无效", body = crate::dto::ErrorResponse),
        (status = 404, description = "资源不存在", body = crate::dto::ErrorResponse),
        (status = 500, description = "服务端错误", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn soft_delete_resource(
    State(state): State<HttpState>,
    access: Extension<AccessContext>,
    Path(id): Path<String>,
) -> Result<Json<ResourceResponse>, HttpError> {
    let id = parse_resource_id(&id)?;

    match state.secured(&access.0).soft_delete_resource(&id).await? {
        Some(resource) => Ok(Json(resource_response(state.service(), &resource)?)),
        None => Err(HttpError::not_found(format!("resource `{id}` not found"))),
    }
}

/// 物理移除资源和对象内容。
#[utoipa::path(
    delete,
    path = "/resources/{id}/purge",
    tag = "resources",
    params(
        ("id" = String, Path, description = "资源 ID")
    ),
    responses(
        (status = 204, description = "资源和对象内容已物理移除"),
        (status = 400, description = "请求参数无效", body = crate::dto::ErrorResponse),
        (status = 404, description = "资源不存在", body = crate::dto::ErrorResponse),
        (status = 500, description = "服务端错误", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn remove_resource(
    State(state): State<HttpState>,
    access: Extension<AccessContext>,
    Path(id): Path<String>,
) -> Result<StatusCode, HttpError> {
    let id = parse_resource_id(&id)?;

    if state.secured(&access.0).remove_resource(&id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(HttpError::not_found(format!("resource `{id}` not found")))
    }
}

pub(super) fn apply_common_resource_fields(
    mut command: CreateResource,
    kind: Option<String>,
    status: Option<String>,
    directory: Option<ResourceDirectory>,
    description: Option<String>,
    tags: Option<Vec<String>>,
) -> Result<CreateResource, HttpError> {
    if let Some(kind) = kind {
        command = command.with_kind(parse_kind(kind)?);
    }

    if let Some(status) = status {
        command = command.with_status(parse_status(&status)?);
    }

    if let Some(directory) = directory {
        command = command.with_directory(directory);
    }

    if let Some(description) = description {
        command = command.with_description(description);
    }

    if let Some(tags) = tags {
        command = command.with_tags(tags);
    }

    Ok(command)
}

pub(super) fn apply_common_stream_fields(
    mut command: UploadResourceContentStream,
    kind: Option<String>,
    status: Option<String>,
    description: Option<String>,
    tags_json: Option<String>,
) -> Result<UploadResourceContentStream, HttpError> {
    if let Some(kind) = kind {
        command = command.with_kind(parse_kind(kind)?);
    }

    if let Some(status) = status {
        command = command.with_status(parse_status(&status)?);
    }

    if let Some(description) = description {
        command = command.with_description(description);
    }

    if let Some(tags_json) = tags_json {
        let tags = serde_json::from_str::<Vec<String>>(&tags_json)
            .map_err(|error| HttpError::bad_request(format!("invalid tags_json: {error}")))?;
        command = command.with_tags(tags);
    }

    Ok(command)
}

pub(super) fn parse_status(value: &str) -> Result<ResourceStatus, HttpError> {
    value
        .trim()
        .to_ascii_lowercase()
        .parse()
        .map_err(|_| HttpError::bad_request(format!("invalid resource status `{value}`")))
}

pub(super) fn parse_kind(value: impl Into<String>) -> Result<ResourceKind, HttpError> {
    ResourceKind::try_new(value.into()).map_err(Into::into)
}

pub(super) fn resource_response(
    service: &asset_core::service::ResourceService,
    resource: &asset_core::domain::Resource,
) -> Result<ResourceResponse, CoreError> {
    let actions = service.describe_resource_actions(resource)?;
    Ok(ResourceResponse::new(resource, actions))
}

pub(super) fn resource_page_response(
    service: &asset_core::service::ResourceService,
    page_result: asset_core::port::ResourcePage,
    page: u32,
) -> Result<ResourcePageResponse, CoreError> {
    Ok(ResourcePageResponse {
        items: page_result
            .items
            .iter()
            .map(|resource| resource_response(service, resource))
            .collect::<Result<Vec<_>, _>>()?,
        total: page_result.total,
        page,
        limit: page_result.limit,
    })
}

pub(super) fn parse_resource_id(value: &str) -> Result<ResourceId, HttpError> {
    ResourceId::from_str(value).map_err(|error| HttpError::bad_request(error.to_string()))
}

pub(super) fn parse_json_payload<T>(
    payload: Result<Json<T>, JsonRejection>,
) -> Result<T, HttpError> {
    payload
        .map(|Json(payload)| payload)
        .map_err(|error| HttpError::bad_request(format!("invalid JSON request body: {error}")))
}
