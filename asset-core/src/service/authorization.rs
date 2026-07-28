use crate::{
    CoreError,
    domain::{AccessContext, DirectoryPath, DirectoryPermission},
    port::{DirectoryLocation, UserRepository},
    service::DirectoryService,
};
use std::sync::Arc;

/// 当前访问主体看到的目录根。外部路径相对于该根，内部服务始终使用真实目录。
#[derive(Debug, Clone)]
pub struct WorkspaceScope {
    root: DirectoryLocation,
}

impl WorkspaceScope {
    pub fn resolve(&self, directory: &DirectoryPath) -> Result<DirectoryPath, CoreError> {
        if self.root.id().is_root() {
            return Ok(directory.clone());
        }
        if directory.is_root() {
            return Ok(self.root.path().clone());
        }
        DirectoryPath::from_path(format!("{}/{}", self.root.path().path(), directory.path()))
            .map_err(Into::into)
    }

    pub fn project(&self, directory: &DirectoryPath) -> Result<DirectoryPath, CoreError> {
        if self.root.id().is_root() {
            return Ok(directory.clone());
        }
        if directory == self.root.path() {
            return Ok(DirectoryPath::root());
        }
        let Some(relative) = directory
            .path()
            .strip_prefix(self.root.path().path())
            .and_then(|suffix| suffix.strip_prefix('/'))
        else {
            return Err(CoreError::forbidden("read", directory.path()));
        };
        DirectoryPath::from_path(relative).map_err(Into::into)
    }

    pub fn root(&self) -> &DirectoryLocation {
        &self.root
    }
}

#[derive(Clone)]
pub struct AuthorizationService {
    users: Arc<dyn UserRepository>,
    directories: DirectoryService,
}

impl AuthorizationService {
    pub fn new(users: Arc<dyn UserRepository>, directories: DirectoryService) -> Self {
        Self { users, directories }
    }
    pub async fn require(
        &self,
        context: &AccessContext,
        directory: &DirectoryLocation,
        permission: DirectoryPermission,
    ) -> Result<(), CoreError> {
        let scope = self.workspace_scope(context).await?;
        if self
            .directories
            .contains(&scope.root().id(), &directory.id())
            .await?
        {
            return Ok(());
        }
        Err(CoreError::forbidden(
            permission_action(permission),
            directory.path().path(),
        ))
    }

    pub async fn workspace_scope(
        &self,
        context: &AccessContext,
    ) -> Result<WorkspaceScope, CoreError> {
        if context.is_administrator() {
            return Ok(WorkspaceScope {
                root: self.directories.root().await?,
            });
        }
        let user = self
            .users
            .find_by_id(&context.user_id())
            .await?
            .ok_or_else(|| CoreError::not_found("user", context.user_id().to_string()))?;
        Ok(WorkspaceScope {
            root: self
                .directories
                .locate_by_id(&user.workspace_directory_id())
                .await?,
        })
    }
}

fn permission_action(permission: DirectoryPermission) -> &'static str {
    match permission {
        DirectoryPermission::Read => "read",
        DirectoryPermission::Write => "write",
        DirectoryPermission::Full => "full",
    }
}

#[cfg(test)]
mod tests;
