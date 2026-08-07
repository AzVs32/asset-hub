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

/// 列出当前后端支持的目录类型。
#[utoipa::path(
    get,
    path = "/directory-kinds",
    tag = "directories",
    responses((status = 200, description = "目录类型列表", body = DirectoryKindsResponse))
)]
pub(crate) async fn list_directory_kinds(
    State(state): State<HttpState>,
) -> Json<DirectoryKindsResponse> {
    let directories = state.service().directory_service();
    Json(DirectoryKindsResponse {
        items: directories
            .kind_definitions()
            .iter()
            .map(|definition| DirectoryKindResponse::from_definition(definition, directories))
            .collect(),
    })
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
    let workspace = state.workspace(&access.0).await?;
    let page = query.page.unwrap_or(DEFAULT_PAGE).max(1);
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset = u64::from(page - 1) * u64::from(limit);
    let mut command = ListResources::new(limit, offset)
        .with_include_deleted(query.include_deleted.unwrap_or(false));

    if let Some(kind) = query.kind {
        command = command.with_kind(parse_kind(kind)?);
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
        &workspace,
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
    let workspace = state.workspace(&access.0).await?;
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

    if let Some(q) = query.q {
        resources_query = resources_query.with_q(q);
    }

    let folders = state
        .secured(&access.0)
        .list_directories(&directory)
        .await?
        .into_iter()
        .map(|directory| directory_response(state.service(), &workspace, &directory))
        .collect::<Result<Vec<_>, _>>()?;
    let current = state.secured(&access.0).find_directory(&directory).await?;
    let resources = state
        .secured(&access.0)
        .list_resources(resources_query)
        .await?;

    Ok(Json(DirectoryListingResponse {
        path: directory,
        directory: directory_response(state.service(), &workspace, &current)?,
        folders,
        resources: resource_page_response(state.service(), &workspace, resources, page)?,
    }))
}

/// 创建一个与存储侧实体对应的空目录。
#[utoipa::path(
    post,
    path = "/directories",
    tag = "resources",
    request_body = CreateDirectoryRequest,
    responses(
        (status = 201, description = "目录已创建", body = DirectoryResponse),
        (status = 400, description = "目录名称无效", body = crate::dto::ErrorResponse),
        (status = 403, description = "没有父目录写权限", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn create_directory(
    State(state): State<HttpState>,
    access: Extension<AccessContext>,
    payload: Result<Json<CreateDirectoryRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<DirectoryResponse>), HttpError> {
    let payload = parse_json_payload(payload)?;
    let workspace = state.workspace(&access.0).await?;
    let directory = state
        .secured(&access.0)
        .create_directory(
            &payload.parent_path,
            payload.name,
            payload
                .kind
                .map(parse_directory_kind)
                .transpose()?
                .unwrap_or_default(),
        )
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(directory_response(state.service(), &workspace, &directory)?),
    ))
}

/// 执行目录插件动作。
#[utoipa::path(
    post,
    path = "/directories/{id}/actions/{action}",
    tag = "directories",
    request_body = ExecuteDirectoryActionRequest,
    params(("id" = String, Path), ("action" = String, Path)),
    responses(
        (status = 200, description = "动作执行结果", body = DirectoryActionOutputResponse),
        (status = 400, description = "目录类型不支持该动作", body = crate::dto::ErrorResponse),
        (status = 404, description = "目录不存在", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn execute_directory_action(
    State(state): State<HttpState>,
    access: Extension<AccessContext>,
    Path((id, action)): Path<(String, String)>,
    payload: Result<Json<ExecuteDirectoryActionRequest>, JsonRejection>,
) -> Result<Json<DirectoryActionOutputResponse>, HttpError> {
    let id = parse_directory_id(&id)?;
    let payload = parse_json_payload(payload)?;
    let output = state
        .secured(&access.0)
        .execute_directory_action(
            &id,
            ExecuteDirectoryAction::new(
                asset_core::domain::ActionId::new(action).map_err(CoreError::from)?,
            )
            .with_input(payload.input),
        )
        .await?;
    Ok(Json(DirectoryActionOutputResponse::from(&output)))
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
    let workspace = state.workspace(&access.0).await?;

    match state.secured(&access.0).find_resource(&id).await? {
        Some(resource) => Ok(Json(resource_response(
            state.service(),
            &workspace,
            &resource,
        )?)),
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
    let workspace = state.workspace(&access.0).await?;
    let mut command = UpdateResource::new();

    if let Some(name) = payload.name {
        command = command.with_name(name);
    }

    if let Some(kind) = payload.kind {
        command = command.with_kind(parse_kind(kind)?);
    }

    if let Some(directory) = payload.directory {
        command = command.with_directory(directory);
    }

    if let Some(restore) = payload.restore {
        command = command.with_restore(restore);
    }

    match state
        .secured(&access.0)
        .update_resource(&id, command)
        .await?
    {
        Some(resource) => Ok(Json(
            resource_snapshot_response(state.service(), &workspace, &resource).await?,
        )),
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
        (status = 409, description = "资源版本已变化", body = crate::dto::ErrorResponse),
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
    let mut command = ExecuteResourceAction::new(
        asset_core::domain::ActionId::new(action).map_err(CoreError::from)?,
    )
    .with_input(payload.input.clone());
    if let Some(expected_revision) = payload.expected_revision {
        command = command.with_expected_revision(expected_revision);
    }
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
    let workspace = state.workspace(&access.0).await?;

    match state.secured(&access.0).soft_delete_resource(&id).await? {
        Some(resource) => Ok(Json(
            resource_snapshot_response(state.service(), &workspace, &resource).await?,
        )),
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

pub(super) fn parse_kind(value: impl Into<String>) -> Result<ResourceKind, HttpError> {
    ResourceKind::try_new(value.into()).map_err(Into::into)
}

pub(super) fn parse_directory_kind(value: impl Into<String>) -> Result<DirectoryKind, HttpError> {
    DirectoryKind::try_new(value.into()).map_err(Into::into)
}

pub(super) fn resource_response(
    service: &asset_core::service::ResourceService,
    workspace: &asset_core::service::WorkspaceScope,
    resource: &asset_core::port::LocatedResource,
) -> Result<ResourceResponse, CoreError> {
    let actions = service.describe_resource_actions(resource.resource())?;
    Ok(ResourceResponse::new(
        resource.resource(),
        workspace.project(resource.directory().path())?,
        actions,
    ))
}

pub(super) async fn resource_snapshot_response(
    service: &asset_core::service::ResourceService,
    workspace: &asset_core::service::WorkspaceScope,
    resource: &asset_core::domain::Resource,
) -> Result<ResourceResponse, CoreError> {
    let actions = service.describe_resource_actions(resource)?;
    let directory = service.locate_resource_directory(resource).await?;
    Ok(ResourceResponse::new(
        resource,
        workspace.project(directory.path())?,
        actions,
    ))
}

pub(super) fn directory_response(
    service: &asset_core::service::ResourceService,
    workspace: &asset_core::service::WorkspaceScope,
    directory: &asset_core::port::LocatedDirectory,
) -> Result<DirectoryResponse, CoreError> {
    let path = workspace.project(directory.path())?;
    let actions = service
        .directory_service()
        .describe_actions(directory.directory())?;
    Ok(DirectoryResponse {
        id: directory.id().to_string(),
        path: path.path().to_owned(),
        parent_path: path.parent_path().to_owned(),
        name: path.name().to_owned(),
        kind: directory.directory().kind().as_str().to_string(),
        actions: actions.into(),
    })
}

pub(super) fn resource_page_response(
    service: &asset_core::service::ResourceService,
    workspace: &asset_core::service::WorkspaceScope,
    page_result: asset_core::port::ResourcePage,
    page: u32,
) -> Result<ResourcePageResponse, CoreError> {
    let mut items = Vec::with_capacity(page_result.items.len());
    for resource in &page_result.items {
        items.push(resource_response(service, workspace, resource)?);
    }
    Ok(ResourcePageResponse {
        items,
        total: page_result.total,
        page,
        limit: page_result.limit,
    })
}

pub(super) fn parse_resource_id(value: &str) -> Result<ResourceId, HttpError> {
    ResourceId::from_str(value).map_err(|error| HttpError::bad_request(error.to_string()))
}

pub(super) fn parse_directory_id(value: &str) -> Result<DirectoryId, HttpError> {
    DirectoryId::from_str(value).map_err(|error| HttpError::bad_request(error.to_string()))
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
