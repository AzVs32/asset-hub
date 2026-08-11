use crate::{
    CoreError,
    domain::{DirectoryPath, User, UserId, UserRole, UserStatus},
    port::{LocatedUser, PasswordHasher, UserQuery, UserRepository},
    service::DirectoryService,
};
use std::sync::Arc;

const MIN_PASSWORD_LEN: usize = 4;

#[derive(Clone)]
pub struct UserService {
    repository: Arc<dyn UserRepository>,
    query: Arc<dyn UserQuery>,
    password_hasher: Arc<dyn PasswordHasher>,
    directories: DirectoryService,
}

impl UserService {
    pub fn new(
        repository: Arc<dyn UserRepository>,
        query: Arc<dyn UserQuery>,
        password_hasher: Arc<dyn PasswordHasher>,
        directories: DirectoryService,
    ) -> Self {
        Self {
            repository,
            query,
            password_hasher,
            directories,
        }
    }
    pub async fn create(
        &self,
        username: &str,
        password: &str,
        role: UserRole,
        workspace_directory: Option<DirectoryPath>,
    ) -> Result<User, CoreError> {
        Self::validate_password(password)?;
        let username = username.trim().to_owned();
        if self.repository.find_by_username(&username).await?.is_some() {
            return Err(CoreError::conflict("username already exists"));
        }
        let workspace_path = match workspace_directory {
            Some(directory) => directory,
            None if role == UserRole::Administrator => DirectoryPath::root(),
            None => DirectoryPath::from_path(format!("users/{username}"))?,
        };
        let workspace_directory = self.directories.ensure_path(&workspace_path).await?;
        let user = User::new(
            username,
            self.password_hasher.hash(password)?,
            role,
            workspace_directory.id(),
        )?;
        self.repository.create(&user).await?;
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
    pub async fn find_by_username(&self, username: &str) -> Result<Option<User>, CoreError> {
        self.repository.find_by_username(username.trim()).await
    }
    pub async fn find_located_by_username(
        &self,
        username: &str,
    ) -> Result<Option<LocatedUser>, CoreError> {
        self.query.find_located_by_username(username.trim()).await
    }
    pub async fn list(&self) -> Result<Vec<LocatedUser>, CoreError> {
        self.query.list_located().await
    }
    pub async fn update_status_by_id(
        &self,
        id: &UserId,
        status: UserStatus,
    ) -> Result<Option<LocatedUser>, CoreError> {
        let Some(located) = self.query.find_located_by_id(id).await? else {
            return Ok(None);
        };
        let (mut user, workspace) = located.into_parts();
        user.change_status(status);
        self.repository.save(&user).await?;
        LocatedUser::new(user, workspace).map(Some)
    }

    pub async fn update_status_by_username(
        &self,
        username: &str,
        status: UserStatus,
    ) -> Result<Option<LocatedUser>, CoreError> {
        let Some(located) = self.query.find_located_by_username(username.trim()).await? else {
            return Ok(None);
        };
        let (mut user, workspace) = located.into_parts();
        user.change_status(status);
        self.repository.save(&user).await?;
        LocatedUser::new(user, workspace).map(Some)
    }

    fn validate_password(password: &str) -> Result<(), CoreError> {
        if password.len() < MIN_PASSWORD_LEN {
            return Err(CoreError::WeakPassword);
        }
        Ok(())
    }

    pub async fn update_password_by_id(
        &self,
        id: &UserId,
        password: &str,
    ) -> Result<Option<User>, CoreError> {
        Self::validate_password(password)?;
        let Some(mut user) = self.repository.find_by_id(id).await? else {
            return Ok(None);
        };
        user.change_credential_hash(self.password_hasher.hash(password)?)?;
        self.repository.save(&user).await?;
        Ok(Some(user))
    }

    pub async fn update_password_by_username(
        &self,
        username: &str,
        password: &str,
    ) -> Result<Option<User>, CoreError> {
        Self::validate_password(password)?;
        let Some(mut user) = self.repository.find_by_username(username.trim()).await? else {
            return Ok(None);
        };
        user.change_credential_hash(self.password_hasher.hash(password)?)?;
        self.repository.save(&user).await?;
        Ok(Some(user))
    }
}
