use crate::{
    CoreError, UserError,
    domain::{DirectoryPath, User, UserId, UserRole, UserStatus},
    port::{PasswordHasher, UserRepository},
    service::DirectoryService,
};
use std::sync::Arc;

const MIN_PASSWORD_LEN: usize = 4;

#[derive(Clone)]
pub struct UserService {
    repository: Arc<dyn UserRepository>,
    password_hasher: Arc<dyn PasswordHasher>,
    directories: DirectoryService,
}

impl UserService {
    pub fn new(
        repository: Arc<dyn UserRepository>,
        password_hasher: Arc<dyn PasswordHasher>,
        directories: DirectoryService,
    ) -> Self {
        Self {
            repository,
            password_hasher,
            directories,
        }
    }
    pub async fn create(
        &self,
        username: impl Into<String>,
        password: &str,
        role: UserRole,
        workspace_directory: Option<DirectoryPath>,
    ) -> Result<User, CoreError> {
        if password.len() < MIN_PASSWORD_LEN {
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
        let workspace_path = match workspace_directory {
            Some(directory) => directory,
            None if role == UserRole::Administrator => DirectoryPath::root(),
            None => DirectoryPath::from_path(format!("users/{}", username.trim()))?,
        };
        let workspace_directory = self.directories.ensure_path(&workspace_path).await?;
        let user = User::new(
            username,
            self.password_hasher.hash(password)?,
            role,
            workspace_directory,
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
    pub async fn update_password(
        &self,
        username: &str,
        password: &str,
    ) -> Result<Option<User>, CoreError> {
        if password.len() < MIN_PASSWORD_LEN {
            return Err(UserError::WeakPassword.into());
        }
        let Some(mut user) = self.repository.find_by_username(username.trim()).await? else {
            return Ok(None);
        };
        user.change_credential_hash(self.password_hasher.hash(password)?)?;
        self.repository.save(&user).await?;
        Ok(Some(user))
    }
    pub async fn count(&self) -> Result<u64, CoreError> {
        self.repository.count().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Directory, DirectoryId, DirectoryRef};
    use crate::port::{DirectoryRepository, DirectoryStorage, PasswordHasher, UserRepository};
    use async_trait::async_trait;
    use std::sync::Mutex;

    struct Users {
        users: Mutex<Vec<User>>,
    }

    #[async_trait]
    impl UserRepository for Users {
        async fn create(&self, user: &User) -> Result<(), CoreError> {
            self.users.lock().unwrap().push(user.clone());
            Ok(())
        }

        async fn save(&self, user: &User) -> Result<(), CoreError> {
            let mut users = self.users.lock().unwrap();
            let saved = users
                .iter_mut()
                .find(|saved| saved.id() == user.id())
                .unwrap();
            *saved = user.clone();
            Ok(())
        }

        async fn find_by_id(&self, id: &UserId) -> Result<Option<User>, CoreError> {
            Ok(self
                .users
                .lock()
                .unwrap()
                .iter()
                .find(|user| user.id() == *id)
                .cloned())
        }

        async fn find_by_username(&self, username: &str) -> Result<Option<User>, CoreError> {
            Ok(self
                .users
                .lock()
                .unwrap()
                .iter()
                .find(|user| user.username() == username)
                .cloned())
        }

        async fn list(&self) -> Result<Vec<User>, CoreError> {
            Ok(self.users.lock().unwrap().clone())
        }

        async fn count(&self) -> Result<u64, CoreError> {
            Ok(self.users.lock().unwrap().len() as u64)
        }
    }

    struct TestPasswordHasher;

    impl PasswordHasher for TestPasswordHasher {
        fn hash(&self, password: &str) -> Result<String, CoreError> {
            Ok(format!("hashed:{password}"))
        }

        fn verify(&self, password: &str, hash: &str) -> Result<bool, CoreError> {
            Ok(hash == format!("hashed:{password}"))
        }
    }

    struct Directories;

    #[async_trait]
    impl DirectoryStorage for Directories {
        async fn ensure_directory(&self, _directory: &DirectoryPath) -> Result<(), CoreError> {
            Ok(())
        }
    }

    #[async_trait]
    impl DirectoryRepository for Directories {
        async fn save_directory(&self, _directory: &Directory) -> Result<(), CoreError> {
            Ok(())
        }
        async fn find_directory(&self, _id: &DirectoryId) -> Result<Option<Directory>, CoreError> {
            Ok(None)
        }
        async fn locate_by_id(&self, id: &DirectoryId) -> Result<Option<DirectoryRef>, CoreError> {
            Ok(id.is_root().then(DirectoryRef::root))
        }
        async fn locate_by_path(
            &self,
            path: &DirectoryPath,
        ) -> Result<Option<DirectoryRef>, CoreError> {
            Ok(Some(if path.is_root() {
                DirectoryRef::root()
            } else {
                DirectoryRef::new(DirectoryId::new(), path.clone())
            }))
        }
        async fn list_children(
            &self,
            _parent_id: &DirectoryId,
        ) -> Result<Vec<DirectoryRef>, CoreError> {
            Ok(Vec::new())
        }
        async fn ensure_path(&self, path: &DirectoryPath) -> Result<DirectoryRef, CoreError> {
            Ok(if path.is_root() {
                DirectoryRef::root()
            } else {
                DirectoryRef::new(DirectoryId::new(), path.clone())
            })
        }
        async fn remove_if_empty(&self, _id: &DirectoryId) -> Result<bool, CoreError> {
            Ok(false)
        }
        async fn is_descendant_or_self(
            &self,
            ancestor_id: &DirectoryId,
            candidate_id: &DirectoryId,
        ) -> Result<bool, CoreError> {
            Ok(ancestor_id == candidate_id)
        }
    }

    fn service() -> UserService {
        let user = User::new(
            "alice",
            "hashed:old-password",
            UserRole::Member,
            DirectoryRef::root(),
        )
        .unwrap();
        let directories = Arc::new(Directories);
        UserService::new(
            Arc::new(Users {
                users: Mutex::new(vec![user]),
            }),
            Arc::new(TestPasswordHasher),
            DirectoryService::new(directories.clone(), directories),
        )
    }

    #[tokio::test]
    async fn updating_password_replaces_the_existing_credential() {
        let service = service();

        let updated = service
            .update_password(" alice ", "new-password")
            .await
            .unwrap()
            .unwrap();

        assert_eq!(updated.credential_hash(), "hashed:new-password");
        assert!(
            service
                .authenticate("alice", "old-password")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            service
                .authenticate("alice", "new-password")
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn updating_password_enforces_the_four_character_minimum() {
        let service = service();

        assert!(matches!(
            service.update_password("alice", "abc").await,
            Err(CoreError::User(UserError::WeakPassword))
        ));
        assert!(
            service
                .update_password("alice", "abcd")
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            service
                .update_password("unknown", "valid-password")
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn creating_users_assigns_role_appropriate_default_workspaces() {
        let service = service();

        let member = service
            .create("bob", "member-password", UserRole::Member, None)
            .await
            .unwrap();
        let administrator = service
            .create("operator", "admin-password", UserRole::Administrator, None)
            .await
            .unwrap();

        assert_eq!(member.workspace_directory().path().path(), "users/bob");
        assert!(administrator.workspace_directory().id().is_root());
    }

    #[tokio::test]
    async fn creating_user_preserves_an_explicit_workspace() {
        let service = service();
        let workspace = DirectoryPath::from_path("teams/bob").unwrap();

        let member = service
            .create(
                "bob",
                "member-password",
                UserRole::Member,
                Some(workspace.clone()),
            )
            .await
            .unwrap();

        assert_eq!(member.workspace_directory().path(), &workspace);
    }
}
