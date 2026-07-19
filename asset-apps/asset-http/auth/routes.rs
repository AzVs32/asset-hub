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
    if let Err(error) = session.backend.check_login_allowed(&username) {
        session
            .backend
            .record_audit(http_audit_event(
                SecurityAuditActor::unauthenticated(),
                SecurityAuditEventType::AuthLogin,
                StatusCode::TOO_MANY_REQUESTS,
                Some(username.clone()),
            ))
            .await;
        return Err(error);
    }
    let user = match session.authenticate(credentials).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            session.backend.record_login_result(&username, false)?;
            session
                .backend
                .record_audit(http_audit_event(
                    SecurityAuditActor::unauthenticated(),
                    SecurityAuditEventType::AuthLogin,
                    StatusCode::UNAUTHORIZED,
                    Some(username.clone()),
                ))
                .await;
            return Err(CoreErrorMarker::Unauthenticated.into());
        }
        Err(error) => {
            session
                .backend
                .record_audit(http_audit_event(
                    SecurityAuditActor::unauthenticated(),
                    SecurityAuditEventType::AuthLogin,
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Some(username.clone()),
                ))
                .await;
            return Err(internal(error));
        }
    };
    session.backend.record_login_result(&username, true)?;
    if let Err(error) = session.login(&user).await {
        session
            .backend
            .record_audit(http_audit_event(
                SecurityAuditActor::authenticated(user.id),
                SecurityAuditEventType::AuthLogin,
                StatusCode::INTERNAL_SERVER_ERROR,
                Some(username.clone()),
            ))
            .await;
        return Err(internal(error));
    }
    session
        .backend
        .record_audit(http_audit_event(
            SecurityAuditActor::authenticated(user.id),
            SecurityAuditEventType::AuthLogin,
            StatusCode::OK,
            Some(username),
        ))
        .await;
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
    let workspace_directory = match request.workspace_directory {
        Some(directory) => directory,
        None if request.is_admin => ResourceDirectory::root(),
        None => {
            return Err(HttpError::bad_request(
                "workspace_directory is required for member users",
            ));
        }
    };
    let user = session
        .backend
        .users
        .create(
            request.username,
            &request.password,
            role,
            workspace_directory,
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
    Ok(Json(
        session
            .backend
            .users
            .list()
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
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
        .update_status(&id, request.status)
        .await?
        .ok_or_else(|| HttpError::not_found(format!("user `{id}` not found")))?;
    Ok(Json(user.into()))
}

#[utoipa::path(
    get,
    path = "/auth/audit-events",
    tag = "authentication",
    params(SecurityAuditQuery),
    responses((status = 200, body = [SecurityAuditEventResponse]), (status = 403))
)]
pub(crate) async fn list_security_audit_events(
    session: Session,
    axum::extract::Query(query): axum::extract::Query<SecurityAuditQuery>,
) -> Result<Json<Vec<SecurityAuditEventResponse>>, HttpError> {
    require_admin(&session)?;
    let page = query.page.unwrap_or(1).max(1);
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let offset = u64::from(page - 1) * u64::from(limit);
    let events = session
        .backend
        .audit
        .list(limit, offset)
        .await
        .map_err(|error| HttpError::internal(format!("list security audit events: {error}")))?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(events))
}

pub(crate) async fn audit_request(
    session: Session,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let method = request.method().clone();
    if matches!(method, Method::GET | Method::HEAD | Method::OPTIONS)
        || request.uri().path() == "/auth/login"
    {
        return next.run(request).await;
    }
    let event_type = security_event_type(&method, request.uri().path());
    let actor = session
        .user
        .as_ref()
        .map_or_else(SecurityAuditActor::unauthenticated, |user| {
            SecurityAuditActor::authenticated(user.id)
        });
    let response = next.run(request).await;
    if let Some(event_type) = event_type {
        session
            .backend
            .record_audit(http_audit_event(actor, event_type, response.status(), None))
            .await;
    }
    response
}

fn http_audit_event(
    actor: SecurityAuditActor,
    event_type: SecurityAuditEventType,
    status: StatusCode,
    target: Option<String>,
) -> NewSecurityAuditEvent {
    NewSecurityAuditEvent {
        actor,
        source: SecurityAuditSource::Http,
        event_type,
        outcome: if status.as_u16() < 400 {
            SecurityAuditOutcome::Success
        } else {
            SecurityAuditOutcome::Failure
        },
        target,
    }
}

fn security_event_type(method: &Method, path: &str) -> Option<SecurityAuditEventType> {
    match (method, path) {
        (&Method::POST, "/auth/logout") => Some(SecurityAuditEventType::AuthLogout),
        (&Method::POST, "/auth/users") => Some(SecurityAuditEventType::AuthUserCreate),
        (&Method::PATCH, path) if path.starts_with("/auth/users/") => {
            Some(SecurityAuditEventType::AuthUserStatus)
        }
        (&Method::POST, "/scan") => Some(SecurityAuditEventType::MaintenanceStorageScan),
        (&Method::DELETE, path) if path.ends_with("/purge") => {
            Some(SecurityAuditEventType::ResourcePurge)
        }
        (&Method::DELETE, path) if path.starts_with("/resources/") => {
            Some(SecurityAuditEventType::ResourceSoftDelete)
        }
        (&Method::POST, path) if path.contains("/actions/") => {
            Some(SecurityAuditEventType::ResourceAction)
        }
        (&Method::PUT, "/resources/content/stream") => Some(SecurityAuditEventType::ResourceUpload),
        (&Method::POST, "/resources") => Some(SecurityAuditEventType::ResourceCreate),
        (&Method::PATCH, path) if path.starts_with("/resources/") => {
            Some(SecurityAuditEventType::ResourceUpdate)
        }
        (&Method::POST, "/directories") => Some(SecurityAuditEventType::DirectoryCreate),
        _ => None,
    }
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
    let public_api_documentation = matches!(request.method(), &Method::GET | &Method::HEAD)
        && (path == "/api-docs/openapi.json"
            || path == "/swagger-ui"
            || path.starts_with("/swagger-ui/"));
    if path == "/health" || public_plugin_asset || public_api_documentation {
        return next.run(request).await;
    }
    let user = match require_user(&session) {
        Ok(user) => user,
        Err(error) => return error.into_response(),
    };
    let context = user.access_context();
    request.extensions_mut().insert(context.clone());
    if user.is_admin || request.uri().path() == "/resource-kinds" {
        return next.run(request).await;
    }
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let permission = if matches!(method, Method::GET | Method::HEAD) {
        DirectoryPermission::Read
    } else {
        DirectoryPermission::Write
    };
    let directory = if path == "/directories" || path == "/resources" {
        query_value(
            request.uri().query().unwrap_or_default(),
            if path == "/directories" {
                "path"
            } else {
                "directory"
            },
        )
    } else {
        None
    };
    let Some(directory) = directory else {
        // Resource-by-id and body-based commands are authorized inside their use case/handler.
        return next.run(request).await;
    };
    let directory = match ResourceDirectory::from_path(directory) {
        Ok(directory) => directory,
        Err(error) => return HttpError::from(asset_core::CoreError::from(error)).into_response(),
    };
    match session
        .backend
        .authorization
        .require(&context, &directory, permission)
        .await
    {
        Ok(()) => next.run(request).await,
        Err(error) => HttpError::from(error).into_response(),
    }
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
fn query_value(query: &str, name: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == name).then(|| value.replace("%2F", "/").replace("%2f", "/"))
    })
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
