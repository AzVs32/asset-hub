use crate::{
    CoreError, UserError,
    domain::{
        DirectoryGrant, DirectoryPermission, ResourceDirectory, User, UserId, UserRole, UserStatus,
    },
    port::{DirectoryStorage, PasswordHasher, UserRepository},
};
use std::sync::Arc;

#[derive(Clone)]
pub struct UserService {
    repository: Arc<dyn UserRepository>,
    password_hasher: Arc<dyn PasswordHasher>,
    directory_storage: Arc<dyn DirectoryStorage>,
}

impl UserService {
    pub fn new(
        repository: Arc<dyn UserRepository>,
        password_hasher: Arc<dyn PasswordHasher>,
        directory_storage: Arc<dyn DirectoryStorage>,
    ) -> Self {
        Self {
            repository,
            password_hasher,
            directory_storage,
        }
    }
    pub async fn create(
        &self,
        username: impl Into<String>,
        password: &str,
        role: UserRole,
        workspace_directory: ResourceDirectory,
    ) -> Result<User, CoreError> {
        if password.len() < 10 {
            return Err(UserError::WeakPassword.into());
        }
        let username = username.into();
        if self
            .repository
            .find_by_username(username.trim())
            .await?
            .is_some()
        {
            return Err(CoreError::conflict("username already exists"));
        }
        let user = User::new(
            username,
            self.password_hasher.hash(password)?,
            role,
            workspace_directory.clone(),
        )?;
        let workspace_grant =
            DirectoryGrant::new(user.id(), workspace_directory, DirectoryPermission::Full);
        self.directory_storage
            .ensure_directory(user.workspace_directory())
            .await?;
        self.repository.create(&user, &workspace_grant).await?;
        Ok(user)
    }
    pub async fn authenticate(
        &self,
        username: &str,
        password: &str,
    ) -> Result<Option<User>, CoreError> {
        let Some(user) = self.repository.find_by_username(username.trim()).await? else {
            return Ok(None);
        };
        if !user.is_active()
            || !self
                .password_hasher
                .verify(password, user.credential_hash())?
        {
            return Ok(None);
        }
        Ok(Some(user))
    }
    pub async fn find_by_id(&self, id: &UserId) -> Result<Option<User>, CoreError> {
        self.repository.find_by_id(id).await
    }
    pub async fn list(&self) -> Result<Vec<User>, CoreError> {
        self.repository.list().await
    }
    pub async fn update_status(
        &self,
        id: &UserId,
        status: UserStatus,
    ) -> Result<Option<User>, CoreError> {
        let Some(mut user) = self.repository.find_by_id(id).await? else {
            return Ok(None);
        };
        user.change_status(status);
        self.repository.save(&user).await?;
        Ok(Some(user))
    }
    pub async fn count(&self) -> Result<u64, CoreError> {
        self.repository.count().await
    }
}
