use asset_core::{
    CoreError,
    domain::{DirectoryId, DirectoryPath, User, UserId, UserRole, UserStatus},
    port::{DirectoryLocation, LocatedUser, UserQuery, UserRepository},
};
use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

const USER_SELECT: &str = r#"
    SELECT users.id, users.username, users.password_hash, users.role, users.status,
           users.workspace_directory_id, users.created_at, users.updated_at
    FROM users
"#;

const LOCATED_USER_SELECT: &str = r#"
    WITH RECURSIVE directory_paths(id, path) AS (
        SELECT id, ''
        FROM directories
        WHERE id = '00000000-0000-0000-0000-000000000000'
        UNION ALL
        SELECT child.id,
               CASE
                   WHEN parent.path = '' THEN child.name
                   ELSE parent.path || '/' || child.name
               END
        FROM directories child
        JOIN directory_paths parent ON child.parent_id = parent.id
    )
    SELECT users.id, users.username, users.password_hash, users.role, users.status,
           users.workspace_directory_id, directory_paths.path AS workspace_directory_path,
           users.created_at, users.updated_at
    FROM users
    JOIN directory_paths ON directory_paths.id = users.workspace_directory_id
"#;

/// SQLite 用户记录；数据库编码只在基础设施层出现。
#[derive(sqlx::FromRow)]
struct UserRow {
    id: String,
    username: String,
    password_hash: String,
    role: String,
    status: String,
    workspace_directory_id: String,
    created_at: String,
    updated_at: String,
}

#[derive(sqlx::FromRow)]
struct LocatedUserRow {
    #[sqlx(flatten)]
    user: UserRow,
    workspace_directory_path: String,
}

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
            .bind(user.workspace_directory_id().to_string())
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
}

#[async_trait::async_trait]
impl UserQuery for SqliteIdentityRepository {
    async fn find_located_by_id(&self, id: &UserId) -> Result<Option<LocatedUser>, CoreError> {
        self.find_located("id", id.to_string()).await
    }

    async fn find_located_by_username(
        &self,
        username: &str,
    ) -> Result<Option<LocatedUser>, CoreError> {
        self.find_located("username", username.to_owned()).await
    }

    async fn list_located(&self) -> Result<Vec<LocatedUser>, CoreError> {
        let rows = sqlx::query_as::<_, LocatedUserRow>(&format!(
            "{LOCATED_USER_SELECT} ORDER BY users.username"
        ))
        .fetch_all(&self.pool)
        .await
        .map_err(|error| CoreError::repository("user.list", error))?;
        rows.into_iter().map(decode_located_user).collect()
    }
}

impl SqliteIdentityRepository {
    async fn find(&self, column: &'static str, value: String) -> Result<Option<User>, CoreError> {
        let sql = match column {
            "id" => format!("{USER_SELECT} WHERE users.id = ?"),
            _ => format!("{USER_SELECT} WHERE users.username = ?"),
        };
        let row = sqlx::query_as::<_, UserRow>(&sql)
            .bind(value)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| CoreError::repository("user.find", e))?;
        row.map(decode_user).transpose()
    }

    async fn find_located(
        &self,
        column: &'static str,
        value: String,
    ) -> Result<Option<LocatedUser>, CoreError> {
        let sql = match column {
            "id" => format!("{LOCATED_USER_SELECT} WHERE users.id = ?"),
            _ => format!("{LOCATED_USER_SELECT} WHERE users.username = ?"),
        };
        let row = sqlx::query_as::<_, LocatedUserRow>(&sql)
            .bind(value)
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| CoreError::repository("user.query", error))?;
        row.map(decode_located_user).transpose()
    }
}

fn decode_user(row: UserRow) -> Result<User, CoreError> {
    User::rehydrate(
        row.id
            .parse()
            .map_err(|error| CoreError::repository("user.decode_id", error))?,
        row.username,
        row.password_hash,
        parse_role(&row.role)?,
        parse_status(&row.status)?,
        decode_workspace_id(&row.workspace_directory_id)?,
        decode_timestamp("user.decode_created_at", &row.created_at)?,
        decode_timestamp("user.decode_updated_at", &row.updated_at)?,
    )
    .map_err(|error| CoreError::repository("user.rehydrate", error))
}

fn decode_located_user(row: LocatedUserRow) -> Result<LocatedUser, CoreError> {
    let workspace_id = decode_workspace_id(&row.user.workspace_directory_id)?;
    let workspace_path = DirectoryPath::from_path(row.workspace_directory_path)
        .map_err(|error| CoreError::repository("user.decode_workspace_path", error))?;
    let user = decode_user(row.user)?;
    LocatedUser::new(user, DirectoryLocation::new(workspace_id, workspace_path))
}

fn decode_workspace_id(value: &str) -> Result<DirectoryId, CoreError> {
    value
        .parse()
        .map_err(|error| CoreError::repository("user.decode_directory_id", error))
}

fn decode_timestamp(operation: &'static str, value: &str) -> Result<DateTime<Utc>, CoreError> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| CoreError::repository(operation, error))
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
        _ => Err(CoreError::repository(
            "user.decode_role",
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown user role `{value}`"),
            ),
        )),
    }
}
fn parse_status(value: &str) -> Result<UserStatus, CoreError> {
    match value {
        "active" => Ok(UserStatus::Active),
        "disabled" => Ok(UserStatus::Disabled),
        _ => Err(CoreError::repository(
            "user.decode_status",
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown user status `{value}`"),
            ),
        )),
    }
}

#[cfg(test)]
mod tests;
