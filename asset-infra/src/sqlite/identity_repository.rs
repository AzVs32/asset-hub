use asset_core::{
    CoreError,
    domain::{
        DirectoryId, DirectoryPath, DirectoryRef, User, UserId, UserRole, UserSnapshot, UserStatus,
    },
    port::UserRepository,
};
use chrono::{DateTime, Utc};
use sqlx::{Row, SqlitePool};
use std::str::FromStr;

const USER_SELECT: &str = r#"
    WITH RECURSIVE directory_paths(id, parent_id, name, path) AS (
        SELECT id, parent_id, name, ''
        FROM directories
        WHERE id = '00000000-0000-0000-0000-000000000000'
        UNION ALL
        SELECT child.id, child.parent_id, child.name,
               CASE WHEN parent.path = '' THEN child.name
                    ELSE parent.path || '/' || child.name END
        FROM directories child
        JOIN directory_paths parent ON child.parent_id = parent.id
    )
    SELECT users.id, users.username, users.password_hash, users.role, users.status,
           users.workspace_directory_id, directory_paths.path AS workspace_directory_path,
           users.created_at, users.updated_at
    FROM users
    JOIN directory_paths ON directory_paths.id = users.workspace_directory_id
"#;

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

        sqlx::query("INSERT INTO users (id, username, password_hash, role, status, workspace_directory_id, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(user.id().to_string())
            .bind(user.username())
            .bind(user.credential_hash())
            .bind(role_to_str(user.role()))
            .bind(status_to_str(user.status()))
            .bind(user.workspace_directory().id().to_string())
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
        let rows = sqlx::query(&format!("{USER_SELECT} ORDER BY users.username"))
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
            "id" => format!("{USER_SELECT} WHERE users.id = ?"),
            _ => format!("{USER_SELECT} WHERE users.username = ?"),
        };
        let row = sqlx::query(&sql)
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
        workspace_directory: DirectoryRef::new(
            DirectoryId::from_str(
                row.try_get::<String, _>("workspace_directory_id")
                    .map_err(|e| CoreError::repository("user.decode", e))?
                    .as_str(),
            )
            .map_err(|e| CoreError::repository("user.decode_directory_id", e))?,
            DirectoryPath::from_path(
                row.try_get::<String, _>("workspace_directory_path")
                    .map_err(|e| CoreError::repository("user.decode", e))?,
            )?,
        ),
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
