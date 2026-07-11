use asset_core::{
    CoreError,
    domain::{
        DirectoryGrant, DirectoryPermission, User, UserId, UserRole, UserSnapshot, UserStatus,
    },
    port::{AccessPolicyRepository, UserRepository},
};
use chrono::{DateTime, Utc};
use sqlx::{Row, SqlitePool};
use std::str::FromStr;

#[derive(Clone)]
pub struct SqliteIdentityRepository {
    pool: SqlitePool,
}

impl SqliteIdentityRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl UserRepository for SqliteIdentityRepository {
    async fn save(&self, user: &User) -> Result<(), CoreError> {
        sqlx::query("INSERT INTO users (id, username, password_hash, is_admin, role, status, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(id) DO UPDATE SET username=excluded.username, password_hash=excluded.password_hash, is_admin=excluded.is_admin, role=excluded.role, status=excluded.status, updated_at=excluded.updated_at")
            .bind(user.id().to_string()).bind(user.username()).bind(user.credential_hash()).bind(user.is_administrator()).bind(role_to_str(user.role())).bind(status_to_str(user.status()))
            .bind(user.created_at().to_rfc3339()).bind(user.updated_at().to_rfc3339()).execute(&self.pool).await
            .map_err(|error| CoreError::repository("user.save", error))?;
        Ok(())
    }
    async fn find_by_id(&self, id: &UserId) -> Result<Option<User>, CoreError> {
        self.find("id", id.to_string()).await
    }
    async fn find_by_username(&self, username: &str) -> Result<Option<User>, CoreError> {
        self.find("username", username.to_owned()).await
    }
    async fn list(&self) -> Result<Vec<User>, CoreError> {
        let rows = sqlx::query(
            "SELECT id, username, password_hash, role, status, created_at, updated_at FROM users ORDER BY username",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| CoreError::repository("user.list", e))?;
        rows.into_iter().map(decode_user).collect()
    }
    async fn count(&self) -> Result<u64, CoreError> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| CoreError::repository("user.count", e))?;
        Ok(count as u64)
    }
}

impl SqliteIdentityRepository {
    async fn find(&self, column: &'static str, value: String) -> Result<Option<User>, CoreError> {
        let sql = match column {
            "id" => {
                "SELECT id, username, password_hash, role, status, created_at, updated_at FROM users WHERE id = ?"
            }
            _ => {
                "SELECT id, username, password_hash, role, status, created_at, updated_at FROM users WHERE username = ?"
            }
        };
        let row = sqlx::query(sql)
            .bind(value)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| CoreError::repository("user.find", e))?;
        row.map(decode_user).transpose()
    }
}

#[async_trait::async_trait]
impl AccessPolicyRepository for SqliteIdentityRepository {
    async fn save_grant(&self, grant: &DirectoryGrant) -> Result<(), CoreError> {
        sqlx::query("INSERT INTO directory_acl (directory_path, user_id, permission, created_at) VALUES (?, ?, ?, ?) ON CONFLICT(directory_path, user_id) DO UPDATE SET permission=excluded.permission")
            .bind(grant.directory()).bind(grant.user_id().to_string()).bind(grant.permission().to_string()).bind(Utc::now().to_rfc3339())
            .execute(&self.pool).await.map_err(|e| CoreError::repository("access.save_grant", e))?;
        Ok(())
    }
    async fn list_grants(&self, user_id: &UserId) -> Result<Vec<DirectoryGrant>, CoreError> {
        let rows = sqlx::query("SELECT directory_path, permission FROM directory_acl WHERE user_id = ? ORDER BY directory_path")
            .bind(user_id.to_string()).fetch_all(&self.pool).await.map_err(|e| CoreError::repository("access.list_grants", e))?;
        rows.into_iter()
            .map(|row| decode_grant(*user_id, row))
            .collect()
    }
    async fn remove_grant(&self, user_id: &UserId, directory: &str) -> Result<(), CoreError> {
        sqlx::query("DELETE FROM directory_acl WHERE user_id = ? AND directory_path = ?")
            .bind(user_id.to_string())
            .bind(directory)
            .execute(&self.pool)
            .await
            .map_err(|e| CoreError::repository("access.remove_grant", e))?;
        Ok(())
    }
    async fn effective_permission(
        &self,
        user_id: &UserId,
        directory: &str,
    ) -> Result<Option<DirectoryPermission>, CoreError> {
        let mut candidate = directory;
        loop {
            let value = sqlx::query_scalar::<_, String>(
                "SELECT permission FROM directory_acl WHERE user_id = ? AND directory_path = ?",
            )
            .bind(user_id.to_string())
            .bind(candidate)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| CoreError::repository("access.effective_permission", e))?;
            if let Some(value) = value {
                return Ok(Some(value.parse()?));
            }
            if candidate.is_empty() {
                return Ok(None);
            }
            candidate = candidate.rsplit_once('/').map_or("", |(parent, _)| parent);
        }
    }
}

fn decode_user(row: sqlx::sqlite::SqliteRow) -> Result<User, CoreError> {
    let timestamp = |name| -> Result<DateTime<Utc>, CoreError> {
        DateTime::parse_from_rfc3339(
            row.try_get::<String, _>(name)
                .map_err(|e| CoreError::repository("user.decode", e))?
                .as_str(),
        )
        .map(|v| v.with_timezone(&Utc))
        .map_err(|e| CoreError::repository("user.decode_timestamp", e))
    };
    User::rehydrate(UserSnapshot {
        id: UserId::from_str(
            row.try_get::<String, _>("id")
                .map_err(|e| CoreError::repository("user.decode", e))?
                .as_str(),
        )
        .map_err(|e| CoreError::repository("user.decode_id", e))?,
        username: row
            .try_get("username")
            .map_err(|e| CoreError::repository("user.decode", e))?,
        credential_hash: row
            .try_get("password_hash")
            .map_err(|e| CoreError::repository("user.decode", e))?,
        role: parse_role(
            row.try_get::<String, _>("role")
                .map_err(|e| CoreError::repository("user.decode", e))?
                .as_str(),
        )?,
        status: parse_status(
            row.try_get::<String, _>("status")
                .map_err(|e| CoreError::repository("user.decode", e))?
                .as_str(),
        )?,
        created_at: timestamp("created_at")?,
        updated_at: timestamp("updated_at")?,
    })
    .map_err(Into::into)
}

fn role_to_str(role: UserRole) -> &'static str {
    match role {
        UserRole::Administrator => "administrator",
        UserRole::Member => "member",
    }
}
fn status_to_str(status: UserStatus) -> &'static str {
    match status {
        UserStatus::Active => "active",
        UserStatus::Disabled => "disabled",
    }
}
fn parse_role(value: &str) -> Result<UserRole, CoreError> {
    match value {
        "administrator" => Ok(UserRole::Administrator),
        "member" => Ok(UserRole::Member),
        _ => Err(CoreError::configuration(format!(
            "unknown user role `{value}`"
        ))),
    }
}
fn parse_status(value: &str) -> Result<UserStatus, CoreError> {
    match value {
        "active" => Ok(UserStatus::Active),
        "disabled" => Ok(UserStatus::Disabled),
        _ => Err(CoreError::configuration(format!(
            "unknown user status `{value}`"
        ))),
    }
}

fn decode_grant(
    user_id: UserId,
    row: sqlx::sqlite::SqliteRow,
) -> Result<DirectoryGrant, CoreError> {
    DirectoryGrant::new(
        user_id,
        row.try_get::<String, _>("directory_path")
            .map_err(|e| CoreError::repository("access.decode", e))?,
        row.try_get::<String, _>("permission")
            .map_err(|e| CoreError::repository("access.decode", e))?
            .parse()?,
    )
    .map_err(Into::into)
}
