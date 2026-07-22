use super::*;
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct AuthenticatedUser {
    #[schema(value_type = String)]
    pub id: UserId,
    pub username: String,
    pub is_admin: bool,
    #[schema(value_type = String)]
    pub workspace_directory: ResourceDirectory,
    #[serde(skip)]
    pub(super) credential_hash: String,
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
    pub(super) fn access_context(&self) -> AccessContext {
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
    pub(super) username: String,
    pub(super) password: String,
}

#[derive(Clone)]
pub(crate) struct AuthBackend {
    pub(super) users: UserService,
    pub(super) authorization: AuthorizationService,
    pub(super) audit: Arc<dyn SecurityAuditRepository>,
    pub(super) login_failures: Arc<Mutex<LoginFailureCache>>,
}

impl AuthBackend {
    pub(crate) fn new(
        users: UserService,
        authorization: AuthorizationService,
        audit: Arc<dyn SecurityAuditRepository>,
    ) -> Self {
        Self {
            users,
            authorization,
            audit,
            login_failures: Arc::new(Mutex::new(LoginFailureCache::default())),
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
                .create(username, password, UserRole::Administrator, None)
                .await?;
        }
        Ok(())
    }

    pub(super) fn check_login_allowed(&self, username: &str) -> Result<(), HttpError> {
        let key = login_failure_key(username);
        let mut failures = self
            .login_failures
            .lock()
            .map_err(|_| HttpError::internal("login rate limiter is unavailable"))?;
        if !failures.check_allowed(&key) {
            return Err(HttpError::too_many_requests(
                "too many failed login attempts; try again later",
            ));
        }
        Ok(())
    }

    pub(super) fn record_login_result(
        &self,
        username: &str,
        succeeded: bool,
    ) -> Result<(), HttpError> {
        let key = login_failure_key(username);
        let mut failures = self
            .login_failures
            .lock()
            .map_err(|_| HttpError::internal("login rate limiter is unavailable"))?;
        failures.record(key, succeeded);
        Ok(())
    }

    pub(super) async fn record_audit(&self, event: NewSecurityAuditEvent) {
        if let Err(error) = self.audit.record(&event).await {
            tracing::error!(error = %error, "write security audit event");
        }
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
