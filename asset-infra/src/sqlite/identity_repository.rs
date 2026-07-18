use asset_core::{
    CoreError,
    domain::{ResourceDirectory, User, UserId, UserRole, UserSnapshot, UserStatus},
    port::UserRepository,
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
    async fn create(&self, user: &User) -> Result<(), CoreError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|error| CoreError::repository("user.create.begin", error))?;

        if !user.workspace_directory().is_root() {
            let now = Utc::now().to_rfc3339();
            let mut parent_path = String::new();
            for name in user.workspace_directory().path().split('/') {
                let path = if parent_path.is_empty() {
                    name.to_owned()
                } else {
                    format!("{parent_path}/{name}")
                };
                sqlx::query(
                    "INSERT INTO directories (path, parent_path, name, created_at, updated_at) VALUES (?, ?, ?, ?, ?) ON CONFLICT(path) DO NOTHING",
                )
                .bind(&path)
                .bind(&parent_path)
                .bind(name)
                .bind(&now)
                .bind(&now)
                .execute(&mut *transaction)
                .await
                .map_err(|error| CoreError::repository("user.create_workspace_directory", error))?;
                parent_path = path;
            }
        }

        sqlx::query("INSERT INTO users (id, username, password_hash, role, status, workspace_directory, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(user.id().to_string())
            .bind(user.username())
            .bind(user.credential_hash())
            .bind(role_to_str(user.role()))
            .bind(status_to_str(user.status()))
            .bind(user.workspace_directory().path())
            .bind(user.created_at().to_rfc3339())
            .bind(user.updated_at().to_rfc3339())
            .execute(&mut *transaction)
            .await
            .map_err(|error| {
                if error.to_string().contains("UNIQUE") {
                    CoreError::conflict("username already exists")
                } else {
                    CoreError::repository("user.create", error)
                }
            })?;

        transaction
            .commit()
            .await
            .map_err(|error| CoreError::repository("user.create.commit", error))?;
        Ok(())
    }

    async fn save(&self, user: &User) -> Result<(), CoreError> {
        sqlx::query("UPDATE users SET username = ?, password_hash = ?, status = ?, updated_at = ? WHERE id = ?")
            .bind(user.username())
            .bind(user.credential_hash())
            .bind(status_to_str(user.status()))
            .bind(user.updated_at().to_rfc3339())
            .bind(user.id().to_string())
            .execute(&self.pool).await
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
            "SELECT id, username, password_hash, role, status, workspace_directory, created_at, updated_at FROM users ORDER BY username",
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
                "SELECT id, username, password_hash, role, status, workspace_directory, created_at, updated_at FROM users WHERE id = ?"
            }
            _ => {
                "SELECT id, username, password_hash, role, status, workspace_directory, created_at, updated_at FROM users WHERE username = ?"
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
        workspace_directory: ResourceDirectory::from_path(
            row.try_get::<String, _>("workspace_directory")
                .map_err(|e| CoreError::repository("user.decode", e))?,
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
