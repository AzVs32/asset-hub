use crate::{
    CoreError, UserError,
    domain::{User, UserId, UserRole, UserStatus},
    port::{PasswordHasher, UserRepository},
};
use std::sync::Arc;

#[derive(Clone)]
pub struct UserService {
    repository: Arc<dyn UserRepository>,
    password_hasher: Arc<dyn PasswordHasher>,
}

impl UserService {
    pub fn new(
        repository: Arc<dyn UserRepository>,
        password_hasher: Arc<dyn PasswordHasher>,
    ) -> Self {
        Self {
            repository,
            password_hasher,
        }
    }
    pub async fn create(
        &self,
        username: impl Into<String>,
        password: &str,
        role: UserRole,
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
        let user = User::new(username, self.password_hasher.hash(password)?, role)?;
        self.repository.save(&user).await?;
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
    pub async fn update_access(
        &self,
        id: &UserId,
        role: UserRole,
        status: UserStatus,
    ) -> Result<Option<User>, CoreError> {
        let Some(mut user) = self.repository.find_by_id(id).await? else {
            return Ok(None);
        };
        user.change_role(role);
        user.change_status(status);
        self.repository.save(&user).await?;
        Ok(Some(user))
    }
    pub async fn count(&self) -> Result<u64, CoreError> {
        self.repository.count().await
    }
}
