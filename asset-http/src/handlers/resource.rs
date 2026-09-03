//! Resource and Directory aggregate HTTP handlers.

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
            .resources()
            .kind_definitions()
            .iter()
            .map(|definition| {
                ResourceKindResponse::from_definition(
                    definition,
                    state.resources(),
                    state.resource_actions(),
                )
            })
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
    let mut command = ListResources::new(limit, offset, DirectoryId::root());

    if let Some(kind) = query.kind {
        command = command.with_kind(parse_kind(kind)?);
    }

    if let Some(q) = query.q {
        command = command.with_q(q);
    }

    let directory = query.directory.unwrap_or_default();

    let page_result = state
        .secured_resources(&access.0)
        .list(&directory, command)
        .await?;

    Ok(Json(resource_page_response(
        state.resources(),
        state.resource_actions(),
        &workspace,
        page_result,
        page,
    )?))
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

    match state
        .secured_resources(&access.0)
        .get(&id)
        .await?
    {
        Some(resource) => Ok(Json(resource_response(
            state.resources(),
            state.resource_actions(),
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
    let mut command = UpdateResource::new(payload.expected_revision);

    if let Some(name) = payload.name {
        command = command.with_name(name);
    }

    if let Some(kind) = payload.kind {
        command = command.with_kind(parse_kind(kind)?);
    }

    if let Some(directory_id) = payload.directory_id {
        command = command.with_directory_id(parse_directory_id(&directory_id)?);
    }

    match state
        .secured_resources(&access.0)
        .update(&id, command)
        .await?
    {
        Some(resource) => Ok(Json(
            resource_snapshot_response(
                state.resources(),
                state.resource_actions(),
                &workspace,
                &resource,
            )
            .await?,
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
        ("action" = String, Path, description = "动作 ID"),
        ("Idempotency-Key" = String, Header, description = "可选的幂等键，重复提交不会重复应用 Host effect")
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
    headers: HeaderMap,
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
        asset_core::domain::ResourceActionId::new(action).map_err(CoreError::from)?,
        payload.expected_revision,
    )
    .with_input(payload.input.clone());
    if let Some(key) = parse_idempotency_key(&headers)? {
        command = command.with_idempotency_key(key);
    }
    let Some(output) = state
        .secured_resource_actions(&access.0)
        .execute(&id, command)
        .await?
    else {
        return Err(HttpError::not_found(format!("resource `{id}` not found")));
    };

    Ok(Json(ResourceActionOutputResponse::from(&output)))
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
        (status = 200, description = "资源已软删除", body = ResourceResponse),
        (status = 400, description = "请求参数无效", body = crate::dto::ErrorResponse),
        (status = 404, description = "资源不存在", body = crate::dto::ErrorResponse),
        (status = 409, description = "资源版本已变化", body = crate::dto::ErrorResponse),
        (status = 500, description = "服务端错误", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn delete_resource(
    State(state): State<HttpState>,
    access: Extension<AccessContext>,
    Path(id): Path<String>,
    Query(query): Query<ExpectedRevisionQuery>,
) -> Result<StatusCode, HttpError> {
    let id = parse_resource_id(&id)?;

    if state
        .secured_resources(&access.0)
        .delete(&id, query.expected_revision)
        .await?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(HttpError::not_found(format!("resource `{id}` not found")))
    }
}

pub(super) fn parse_kind(value: impl Into<String>) -> Result<ResourceKind, HttpError> {
    ResourceKind::try_new(value.into()).map_err(|error| CoreError::from(error).into())
}

pub(super) fn resource_response(
    _service: &asset_core::service::ResourceService,
    action_orchestrator: &asset_core::service::ActionOrchestrator,
    workspace: &asset_core::service::WorkspaceScope,
    resource: &asset_core::port::LocatedResource,
) -> Result<ResourceResponse, CoreError> {
    let actions = action_orchestrator.describe_resource_actions(resource.resource())?;
    Ok(ResourceResponse::new(
        resource.resource(),
        workspace.project(resource.directory().path())?,
        actions,
    ))
}

pub(super) async fn resource_snapshot_response(
    service: &asset_core::service::ResourceService,
    action_orchestrator: &asset_core::service::ActionOrchestrator,
    workspace: &asset_core::service::WorkspaceScope,
    resource: &asset_core::domain::Resource,
) -> Result<ResourceResponse, CoreError> {
    let actions = action_orchestrator.describe_resource_actions(resource)?;
    let directory = service.locate_resource_directory(resource).await?;
    Ok(ResourceResponse::new(
        resource,
        workspace.project(directory.path())?,
        actions,
    ))
}

pub(super) fn resource_page_response(
    service: &asset_core::service::ResourceService,
    action_orchestrator: &asset_core::service::ActionOrchestrator,
    workspace: &asset_core::service::WorkspaceScope,
    page_result: asset_core::port::ResourcePage,
    page: u32,
) -> Result<ResourcePageResponse, CoreError> {
    let mut items = Vec::with_capacity(page_result.items.len());
    for resource in &page_result.items {
        items.push(resource_response(
            service,
            action_orchestrator,
            workspace,
            resource,
        )?);
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
