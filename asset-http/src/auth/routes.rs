use super::*;

#[utoipa::path(
    post,
    path = "/auth/login",
    tag = "authentication",
    security(()),
    request_body = Credentials,
    responses(
        (status = 200, body = MeResponse),
        (status = 401, description = "Invalid credentials"),
        (status = 429, description = "Too many failed attempts")
    )
)]
pub(crate) async fn login(
    mut session: Session,
    Json(credentials): Json<Credentials>,
) -> Result<Json<MeResponse>, HttpError> {
    let username = credentials.username.clone();
    session.backend.check_login_allowed(&username)?;
    let user = match session.authenticate(credentials).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            session.backend.record_login_result(&username, false)?;
            return Err(CoreErrorMarker::Unauthenticated.into());
        }
        Err(error) => return Err(internal(error)),
    };
    session.backend.record_login_result(&username, true)?;
    if let Err(error) = session.login(&user).await {
        return Err(internal(error));
    }
    Ok(Json(MeResponse { user }))
}

#[utoipa::path(post, path = "/auth/logout", tag = "authentication", responses((status = 204)))]
pub(crate) async fn logout(mut session: Session) -> Result<StatusCode, HttpError> {
    session.logout().await.map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(get, path = "/auth/me", tag = "authentication", responses((status = 200, body = MeResponse)))]
pub(crate) async fn me(session: Session) -> Result<Json<MeResponse>, HttpError> {
    Ok(Json(MeResponse {
        user: require_user(&session)?.clone(),
    }))
}

#[utoipa::path(
    post,
    path = "/auth/users",
    tag = "authentication",
    request_body = CreateUserRequest,
    responses((status = 201, body = MeResponse), (status = 403))
)]
pub(crate) async fn create_user(
    session: Session,
    Json(request): Json<CreateUserRequest>,
) -> Result<(StatusCode, Json<MeResponse>), HttpError> {
    require_admin(&session)?;
    let role = if request.is_admin {
        UserRole::Administrator
    } else {
        UserRole::Member
    };
    let user = session
        .backend
        .users
        .create(
            &request.username,
            &request.password,
            role,
            request.workspace_directory,
        )
        .await?;
    Ok((StatusCode::CREATED, Json(MeResponse { user: user.into() })))
}

#[utoipa::path(
    get,
    path = "/auth/users",
    tag = "authentication",
    responses((status = 200, body = [ManagedUserResponse]), (status = 403))
)]
pub(crate) async fn list_users(
    session: Session,
) -> Result<Json<Vec<ManagedUserResponse>>, HttpError> {
    require_admin(&session)?;
    let users = session.backend.users.list().await?;
    let response = users.into_iter().map(ManagedUserResponse::new).collect();
    Ok(Json(response))
}

#[utoipa::path(
    patch,
    path = "/auth/users/{id}",
    tag = "authentication",
    params(("id" = String, Path)),
    request_body = UpdateUserStatusRequest,
    responses((status = 200, body = ManagedUserResponse), (status = 403), (status = 404))
)]
pub(crate) async fn update_user_status(
    session: Session,
    axum::extract::Path(id): axum::extract::Path<UserId>,
    Json(request): Json<UpdateUserStatusRequest>,
) -> Result<Json<ManagedUserResponse>, HttpError> {
    let actor = require_user(&session)?;
    require_admin(&session)?;
    if actor.id == id && request.status != UserStatus::Active {
        return Err(HttpError::bad_request(
            "an administrator cannot disable their own account",
        ));
    }
    let user = session
        .backend
        .users
        .update_status_by_id(&id, request.status)
        .await?
        .ok_or_else(|| HttpError::not_found(format!("user `{id}` not found")))?;
    Ok(Json(ManagedUserResponse::new(user)))
}

pub(crate) async fn authorize_request(
    session: Session,
    mut request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let path = request.uri().path();
    // Plugin frames use an opaque sandbox origin; only immutable, verified Web snapshots are public.
    let public_plugin_asset =
        path.starts_with("/plugins/") && matches!(request.method(), &Method::GET | &Method::HEAD);
    let public_api_contract = matches!(request.method(), &Method::GET | &Method::HEAD)
        && path == "/api-docs/openapi.json";
    if path == "/health" || public_plugin_asset || public_api_contract {
        return next.run(request).await;
    }
    let user = match require_user(&session) {
        Ok(user) => user,
        Err(error) => return error.into_response(),
    };
    let context = user.access_context();
    request.extensions_mut().insert(context);
    next.run(request).await
}

fn require_user(session: &Session) -> Result<&AuthenticatedUser, HttpError> {
    session
        .user
        .as_ref()
        .ok_or_else(|| HttpError::unauthorized("login required"))
}
fn require_admin(session: &Session) -> Result<(), HttpError> {
    if require_user(session)?.is_admin {
        Ok(())
    } else {
        Err(HttpError::forbidden("administrator permission required"))
    }
}
fn internal(error: impl std::fmt::Display) -> HttpError {
    HttpError::internal(format!("session error: {error}"))
}

enum CoreErrorMarker {
    Unauthenticated,
}

impl From<CoreErrorMarker> for HttpError {
    fn from(_: CoreErrorMarker) -> Self {
        HttpError::unauthorized("invalid username or password")
    }
}
