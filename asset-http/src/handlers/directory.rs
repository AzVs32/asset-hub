//! Directory aggregate HTTP handlers.

use super::*;

const DEFAULT_PAGE: u32 = 1;
const DEFAULT_LIMIT: u32 = 50;
const MAX_LIMIT: u32 = 100;

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
    let directories = state.directories();
    Json(DirectoryKindsResponse {
        items: directories
            .kind_definitions()
            .iter()
            .map(|definition| {
                DirectoryKindResponse::from_definition(
                    definition,
                    directories,
                    state.resource_actions(),
                )
            })
            .collect(),
    })
}

/// 列出当前目录的直接子目录和资源。
#[utoipa::path(
    get,
    path = "/directories",
    tag = "directories",
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
    let mut resources_query = ListResources::new(limit, offset, DirectoryId::root());

    if let Some(kind) = query.kind {
        resources_query = resources_query.with_kind(parse_kind(kind)?);
    }

    if let Some(q) = query.q {
        resources_query = resources_query.with_q(q);
    }

    let folders = state
        .secured_directories(&access.0)
        .list_children(&directory)
        .await?
        .into_iter()
        .map(|directory| directory_response(state.resource_actions(), &workspace, &directory))
        .collect::<Result<Vec<_>, _>>()?;
    let current = state
        .secured_directories(&access.0)
        .find_by_path(&directory)
        .await?;
    let resources = state
        .secured_resources(&access.0)
        .list(&directory, resources_query)
        .await?;

    Ok(Json(DirectoryListingResponse {
        path: directory,
        directory: directory_response(state.resource_actions(), &workspace, &current)?,
        folders,
        resources: resource_page_response(
            state.resources(),
            state.resource_actions(),
            &workspace,
            resources,
            page,
        )?,
    }))
}

/// 创建一个与存储侧实体对应的空目录。
#[utoipa::path(
    post,
    path = "/directories",
    tag = "directories",
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
    let parent_id = parse_directory_id(&payload.parent_id)?;
    let directory = state
        .secured_directories(&access.0)
        .create(
            &parent_id,
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
        Json(directory_response(
            state.resource_actions(),
            &workspace,
            &directory,
        )?),
    ))
}

/// 按稳定 ID 查询目录。
#[utoipa::path(
    get,
    path = "/directories/{id}",
    tag = "directories",
    params(("id" = String, Path, description = "目录 ID")),
    responses(
        (status = 200, description = "目录详情", body = DirectoryResponse),
        (status = 400, description = "目录 ID 无效", body = crate::dto::ErrorResponse),
        (status = 403, description = "没有目录读取权限", body = crate::dto::ErrorResponse),
        (status = 404, description = "目录不存在", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn find_directory(
    State(state): State<HttpState>,
    access: Extension<AccessContext>,
    Path(id): Path<String>,
) -> Result<Json<DirectoryResponse>, HttpError> {
    let id = parse_directory_id(&id)?;
    let workspace = state.workspace(&access.0).await?;
    let directory = state.secured_directories(&access.0).find_by_id(&id).await?;
    Ok(Json(directory_response(
        state.resource_actions(),
        &workspace,
        &directory,
    )?))
}

/// 以乐观并发方式更新目录元数据或父目录。
#[utoipa::path(
    patch,
    path = "/directories/{id}",
    tag = "directories",
    params(("id" = String, Path, description = "目录 ID")),
    request_body = UpdateDirectoryRequest,
    responses(
        (status = 200, description = "目录已更新", body = DirectoryResponse),
        (status = 400, description = "请求参数无效", body = crate::dto::ErrorResponse),
        (status = 403, description = "没有目录写权限", body = crate::dto::ErrorResponse),
        (status = 404, description = "目录或父目录不存在", body = crate::dto::ErrorResponse),
        (status = 409, description = "目录版本冲突或目标位置冲突", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn update_directory(
    State(state): State<HttpState>,
    access: Extension<AccessContext>,
    Path(id): Path<String>,
    payload: Result<Json<UpdateDirectoryRequest>, JsonRejection>,
) -> Result<Json<DirectoryResponse>, HttpError> {
    let id = parse_directory_id(&id)?;
    let payload = parse_json_payload(payload)?;
    let workspace = state.workspace(&access.0).await?;
    let mut command = UpdateDirectory::new(payload.expected_revision);
    if let Some(name) = payload.name {
        command = command.with_name(name);
    }
    if let Some(parent_id) = payload.parent_id {
        command = command.with_parent_id(parse_directory_id(&parent_id)?);
    }
    if let Some(kind) = payload.kind {
        command = command.with_kind(parse_directory_kind(kind)?);
    }
    let directory = state
        .secured_directories(&access.0)
        .update(&id, command)
        .await?;
    Ok(Json(directory_response(
        state.resource_actions(),
        &workspace,
        &directory,
    )?))
}

/// 删除空目录。根目录和非空目录不可删除。
#[utoipa::path(
    delete,
    path = "/directories/{id}",
    tag = "directories",
    params(
        ("id" = String, Path, description = "目录 ID"),
        ExpectedRevisionQuery
    ),
    responses(
        (status = 204, description = "空目录已删除"),
        (status = 400, description = "目录 ID 无效", body = crate::dto::ErrorResponse),
        (status = 403, description = "没有目录删除权限", body = crate::dto::ErrorResponse),
        (status = 404, description = "目录不存在", body = crate::dto::ErrorResponse),
        (status = 409, description = "目录非空或版本已变化", body = crate::dto::ErrorResponse)
    )
)]
pub(crate) async fn delete_directory(
    State(state): State<HttpState>,
    access: Extension<AccessContext>,
    Path(id): Path<String>,
    Query(query): Query<ExpectedRevisionQuery>,
) -> Result<StatusCode, HttpError> {
    let id = parse_directory_id(&id)?;
    if state
        .secured_directories(&access.0)
        .delete(&id, query.expected_revision)
        .await?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(CoreError::conflict(format!("directory `{id}` is not empty")).into())
    }
}

/// 执行目录插件动作。
#[utoipa::path(
    post,
    path = "/directories/{id}/actions/{action}",
    tag = "directories",
    request_body = ExecuteDirectoryActionRequest,
    params(
        ("id" = String, Path),
        ("action" = String, Path),
        ("Idempotency-Key" = Option<String>, Header, description = "可选的幂等键，重复提交不会重复应用 Host effect")
    ),
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
    headers: HeaderMap,
    payload: Result<Json<ExecuteDirectoryActionRequest>, JsonRejection>,
) -> Result<Json<DirectoryActionOutputResponse>, HttpError> {
    let id = parse_directory_id(&id)?;
    let payload = parse_json_payload(payload)?;
    let mut command = ExecuteDirectoryAction::new(
        asset_core::domain::DirectoryActionId::new(action).map_err(CoreError::from)?,
        payload.expected_revision,
    )
    .with_input(payload.input);
    if let Some(key) = parse_idempotency_key(&headers)? {
        command = command.with_idempotency_key(key);
    }
    let output = state
        .secured_asset_coordination(&access.0)
        .execute_directory_action(&id, command)
        .await?;
    Ok(Json(DirectoryActionOutputResponse::from(&output)))
}

pub(super) fn parse_directory_kind(value: impl Into<String>) -> Result<DirectoryKind, HttpError> {
    DirectoryKind::try_new(value.into()).map_err(|error| CoreError::from(error).into())
}

pub(super) fn directory_response(
    orchestrator: &asset_core::service::ActionOrchestrator,
    workspace: &asset_core::service::WorkspaceScope,
    directory: &asset_core::port::LocatedDirectory,
) -> Result<DirectoryResponse, CoreError> {
    let path = workspace.project(directory.path())?;
    let actions = orchestrator.describe_directory_actions(directory.directory())?;
    Ok(DirectoryResponse {
        id: directory.id().to_string(),
        parent_id: directory.directory().parent_id().map(|id| id.to_string()),
        path: path.path().to_owned(),
        parent_path: path.parent_path().to_owned(),
        name: path.name().to_owned(),
        kind: directory.directory().kind().as_str().to_string(),
        actions: actions
            .available_actions()
            .iter()
            .map(crate::dto::DirectoryActionDefinitionResponse::from)
            .collect(),
        created_at: directory.directory().created_at().to_rfc3339(),
        updated_at: directory.directory().updated_at().to_rfc3339(),
        revision: directory.directory().revision(),
    })
}

pub(super) fn parse_directory_id(value: &str) -> Result<DirectoryId, HttpError> {
    DirectoryId::from_str(value).map_err(|error| HttpError::bad_request(error.to_string()))
}
