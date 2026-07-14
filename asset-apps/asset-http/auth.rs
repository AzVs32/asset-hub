use crate::error::HttpError;
use asset_core::domain::{
    AccessContext, DirectoryGrant, DirectoryPermission, ResourceDirectory, User, UserId, UserRole,
    UserStatus,
};
use asset_core::service::{AuthorizationService, UserService};
use axum::{
    Json,
    http::{Method, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use axum_login::{AuthSession, AuthUser, AuthnBackend};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use utoipa::ToSchema;

const MAX_LOGIN_FAILURES: u8 = 5;
const LOGIN_FAILURE_WINDOW: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct AuthenticatedUser {
    #[schema(value_type = String)]
    pub id: UserId,
    pub username: String,
    pub is_admin: bool,
    #[schema(value_type = String)]
    pub workspace_directory: ResourceDirectory,
    #[serde(skip)]
    credential_hash: String,
}

impl From<User> for AuthenticatedUser {
    fn from(user: User) -> Self {
        Self {
            id: user.id(),
            username: user.username().to_owned(),
            is_admin: user.is_administrator(),
            workspace_directory: user.workspace_directory().clone(),
            credential_hash: user.credential_hash().to_owned(),
        }
    }
}

impl AuthenticatedUser {
    fn access_context(&self) -> AccessContext {
        if self.is_admin {
            AccessContext::administrator(self.id)
        } else {
            AccessContext::member(self.id)
        }
    }
}

impl AuthUser for AuthenticatedUser {
    type Id = UserId;
    fn id(&self) -> Self::Id {
        self.id
    }
    fn session_auth_hash(&self) -> &[u8] {
        self.credential_hash.as_bytes()
    }
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub(crate) struct Credentials {
    username: String,
    password: String,
}

#[derive(Clone)]
pub(crate) struct AuthBackend {
    users: UserService,
    authorization: AuthorizationService,
    login_failures: Arc<Mutex<HashMap<String, LoginFailureState>>>,
}

#[derive(Debug)]
struct LoginFailureState {
    failures: u8,
    started_at: Instant,
}

impl AuthBackend {
    pub(crate) fn new(users: UserService, authorization: AuthorizationService) -> Self {
        Self {
            users,
            authorization,
            login_failures: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    pub(crate) async fn initialize(
        &self,
        bootstrap: Option<(&str, &str)>,
    ) -> Result<(), HttpError> {
        if self.users.count().await? == 0 {
            let Some((username, password)) = bootstrap else {
                return Err(HttpError::internal(
                    "no users exist; set ASSET_HUB_BOOTSTRAP_ADMIN_USERNAME and ASSET_HUB_BOOTSTRAP_ADMIN_PASSWORD for the first startup",
                ));
            };
            self.users
                .create(
                    username,
                    password,
                    UserRole::Administrator,
                    ResourceDirectory::root(),
                )
                .await?;
        }
        Ok(())
    }

    fn check_login_allowed(&self, username: &str) -> Result<(), HttpError> {
        let key = username.trim().to_ascii_lowercase();
        let mut failures = self
            .login_failures
            .lock()
            .map_err(|_| HttpError::internal("login rate limiter is unavailable"))?;
        if failures
            .get(&key)
            .is_some_and(|state| state.started_at.elapsed() >= LOGIN_FAILURE_WINDOW)
        {
            failures.remove(&key);
        }
        if failures
            .get(&key)
            .is_some_and(|state| state.failures >= MAX_LOGIN_FAILURES)
        {
            return Err(HttpError::too_many_requests(
                "too many failed login attempts; try again later",
            ));
        }
        Ok(())
    }

    fn record_login_result(&self, username: &str, succeeded: bool) -> Result<(), HttpError> {
        let key = username.trim().to_ascii_lowercase();
        let mut failures = self
            .login_failures
            .lock()
            .map_err(|_| HttpError::internal("login rate limiter is unavailable"))?;
        if succeeded {
            failures.remove(&key);
        } else {
            let state = failures.entry(key).or_insert(LoginFailureState {
                failures: 0,
                started_at: Instant::now(),
            });
            state.failures = state.failures.saturating_add(1);
        }
        Ok(())
    }
}

impl AuthnBackend for AuthBackend {
    type User = AuthenticatedUser;
    type Credentials = Credentials;
    type Error = asset_core::CoreError;
    async fn authenticate(
        &self,
        credentials: Credentials,
    ) -> Result<Option<Self::User>, Self::Error> {
        Ok(self
            .users
            .authenticate(&credentials.username, &credentials.password)
            .await?
            .map(Into::into))
    }
    async fn get_user(&self, id: &UserId) -> Result<Option<Self::User>, Self::Error> {
        Ok(self
            .users
            .find_by_id(id)
            .await?
            .filter(User::is_active)
            .map(Into::into))
    }
}

pub(crate) type Session = AuthSession<AuthBackend>;

#[derive(Serialize, ToSchema)]
pub(crate) struct MeResponse {
    user: AuthenticatedUser,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct ManagedUserResponse {
    #[schema(value_type = String)]
    id: UserId,
    username: String,
    #[schema(value_type = String)]
    role: UserRole,
    #[schema(value_type = String)]
    status: UserStatus,
    #[schema(value_type = String)]
    workspace_directory: ResourceDirectory,
}

impl From<User> for ManagedUserResponse {
    fn from(user: User) -> Self {
        Self {
            id: user.id(),
            username: user.username().to_owned(),
            role: user.role(),
            status: user.status(),
            workspace_directory: user.workspace_directory().clone(),
        }
    }
}

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
    session.backend.check_login_allowed(&credentials.username)?;
    let username = credentials.username.clone();
    let user = session
        .authenticate(credentials)
        .await
        .map_err(internal)?
        .ok_or(CoreErrorMarker::Unauthenticated);
    session
        .backend
        .record_login_result(&username, user.is_ok())?;
    let user = user?;
    session.login(&user).await.map_err(internal)?;
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

#[derive(Deserialize, ToSchema)]
pub(crate) struct CreateUserRequest {
    username: String,
    password: String,
    #[serde(default)]
    is_admin: bool,
    #[schema(value_type = Option<String>)]
    workspace_directory: Option<ResourceDirectory>,
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

#[derive(Deserialize, ToSchema)]
pub(crate) struct UpdateUserStatusRequest {
    #[schema(value_type = String)]
    status: UserStatus,
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

#[derive(Deserialize, ToSchema)]
pub(crate) struct GrantDirectoryRequest {
    #[schema(value_type = String)]
    user_id: UserId,
    #[schema(value_type = String)]
    directory: ResourceDirectory,
    #[schema(value_type = String)]
    permission: DirectoryPermission,
}

#[utoipa::path(
    put,
    path = "/auth/directory-grants",
    tag = "authentication",
    request_body = GrantDirectoryRequest,
    responses((status = 204), (status = 403))
)]
pub(crate) async fn grant_directory(
    session: Session,
    Json(request): Json<GrantDirectoryRequest>,
) -> Result<StatusCode, HttpError> {
    require_admin(&session)?;
    let actor = require_user(&session)?.access_context();
    let grant = DirectoryGrant::new(request.user_id, request.directory, request.permission);
    session.backend.authorization.grant(&actor, grant).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, ToSchema)]
pub(crate) struct DirectoryGrantResponse {
    #[schema(value_type = String)]
    directory: ResourceDirectory,
    #[schema(value_type = String)]
    permission: DirectoryPermission,
    is_workspace: bool,
}

#[derive(Deserialize, utoipa::IntoParams)]
pub(crate) struct DirectoryGrantQuery {
    #[param(value_type = Option<String>)]
    user_id: Option<UserId>,
}

#[utoipa::path(
    get,
    path = "/auth/directory-grants",
    tag = "authentication",
    params(DirectoryGrantQuery),
    responses((status = 200, body = [DirectoryGrantResponse]))
)]
pub(crate) async fn my_directory_grants(
    session: Session,
    axum::extract::Query(query): axum::extract::Query<DirectoryGrantQuery>,
) -> Result<Json<Vec<DirectoryGrantResponse>>, HttpError> {
    let user = require_user(&session)?;
    let access = user.access_context();
    let target_id = query.user_id.unwrap_or(user.id);
    let grants = session
        .backend
        .authorization
        .grants_for(&access, target_id)
        .await?;
    let target = session
        .backend
        .users
        .find_by_id(&target_id)
        .await?
        .ok_or_else(|| HttpError::not_found(format!("user `{target_id}` not found")))?;
    Ok(Json(
        grants
            .into_iter()
            .map(|grant| DirectoryGrantResponse {
                directory: grant.directory().clone(),
                permission: grant.permission(),
                is_workspace: grant.directory() == target.workspace_directory(),
            })
            .collect(),
    ))
}

#[derive(Deserialize, utoipa::IntoParams)]
pub(crate) struct RevokeDirectoryGrantQuery {
    #[param(value_type = String)]
    user_id: UserId,
    #[param(value_type = String)]
    directory: ResourceDirectory,
}

#[utoipa::path(
    delete,
    path = "/auth/directory-grants",
    tag = "authentication",
    params(RevokeDirectoryGrantQuery),
    responses((status = 204), (status = 403))
)]
pub(crate) async fn revoke_directory(
    session: Session,
    axum::extract::Query(query): axum::extract::Query<RevokeDirectoryGrantQuery>,
) -> Result<StatusCode, HttpError> {
    require_admin(&session)?;
    let actor = require_user(&session)?.access_context();
    session
        .backend
        .authorization
        .revoke(&actor, query.user_id, &query.directory)
        .await?;
    Ok(StatusCode::NO_CONTENT)
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
    if path == "/health" || public_plugin_asset {
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
