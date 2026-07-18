use crate::{config::DatabaseConfig, migration};
use asset_core::CoreError;
use asset_core::domain::{
    Resource, ResourceContent, ResourceDirectory, ResourceId, ResourceKind, ResourceSnapshot,
    ResourceStatus,
};
use asset_core::port::{ListResources, ResourcePage, ResourceQuery, ResourceRepository};
use chrono::{DateTime, Utc};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow};
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool, Transaction};

const RESOURCE_SELECT: &str = r#"
    SELECT
        resources.id,
        resources.name,
        resources.directory,
        resources.kind,
        resources.status,
        resources.description,
        COALESCE(
            (
                SELECT json_group_array(tag)
                FROM (
                    SELECT tags.name AS tag
                    FROM resource_tags
                    JOIN tags ON tags.id = resource_tags.tag_id
                    WHERE resource_tags.resource_id = resources.id
                    ORDER BY tags.name COLLATE BINARY
                )
            ),
            '[]'
        ) AS tags_json,
        resources.content_json,
        resources.created_at,
        resources.updated_at,
        resources.deleted_at
    FROM resources
"#;

/// SQLite 版本的资源聚合仓储。
///
/// 该实现负责把 `Resource` 聚合保存到本地 SQLite 数据库。它是 `asset-core` 中
/// `ResourceRepository` 端口的基础设施适配器。
#[derive(Clone)]
pub struct SqliteResourceRepository {
    pool: SqlitePool,
}

impl SqliteResourceRepository {
    /// 连接 SQLite，并执行尚未应用的数据库迁移。
    pub async fn connect(config: &DatabaseConfig) -> Result<Self, CoreError> {
        if config.max_connections == 0 {
            return Err(CoreError::configuration(
                "database.max_connections must be greater than 0",
            ));
        }

        if let Some(parent) = config.sqlite_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| CoreError::repository("sqlite.create_dir", error))?;
        }

        let options = SqliteConnectOptions::new()
            .filename(&config.sqlite_path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal);

        let pool = SqlitePoolOptions::new()
            .max_connections(config.max_connections)
            .connect_with(options)
            .await
            .map_err(|error| CoreError::repository("sqlite.connect", error))?;

        let repository = Self { pool };
        repository.run_migrations().await?;

        Ok(repository)
    }

    /// 使用已有连接池创建仓储。
    ///
    /// 该构造函数不会自动执行迁移，主要用于测试或由外部迁移系统管理 schema 的场景。
    pub fn from_pool(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// 返回内部 SQLite 连接池。
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// 执行 SQLite 数据库迁移。
    pub async fn run_migrations(&self) -> Result<(), CoreError> {
        migration::sqlite::run(&self.pool).await
    }
}

#[async_trait::async_trait]
impl ResourceRepository for SqliteResourceRepository {
    async fn health_check(&self) -> Result<(), CoreError> {
        sqlx::query_scalar::<_, i64>("SELECT 1")
            .fetch_one(&self.pool)
            .await
            .map(|_| ())
            .map_err(|error| CoreError::repository("health_check", error))
    }

    async fn save(&self, resource: &Resource) -> Result<(), CoreError> {
        ensure_directory_path(&self.pool, resource.directory().path()).await?;
        let content_json = resource
            .content()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|error| CoreError::repository("resource.encode_content", error))?;

        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|error| CoreError::repository("save.begin", error))?;
        sqlx::query(
            r#"
            INSERT INTO resources (
                id,
                name,
                directory,
                kind,
                status,
                description,
                content_json,
                created_at,
                updated_at,
                deleted_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                directory = excluded.directory,
                kind = excluded.kind,
                status = excluded.status,
                description = excluded.description,
                content_json = excluded.content_json,
                created_at = excluded.created_at,
                updated_at = excluded.updated_at,
                deleted_at = excluded.deleted_at
            "#,
        )
        .bind(resource.id().to_string())
        .bind(resource.name())
        .bind(resource.directory().path())
        .bind(resource.kind().as_str())
        .bind(resource.status().as_str())
        .bind(resource.description())
        .bind(content_json)
        .bind(encode_timestamp(resource.created_at()))
        .bind(encode_timestamp(resource.updated_at()))
        .bind(resource.deleted_at().map(encode_timestamp))
        .execute(&mut *transaction)
        .await
        .map_err(|error| CoreError::repository("save", error))?;

        sync_tags(&mut transaction, resource).await?;
        transaction
            .commit()
            .await
            .map_err(|error| CoreError::repository("save.commit", error))?;

        Ok(())
    }

    async fn save_if_unchanged(
        &self,
        resource: &Resource,
        expected_updated_at: DateTime<Utc>,
    ) -> Result<bool, CoreError> {
        ensure_directory_path(&self.pool, resource.directory().path()).await?;
        let content_json = resource
            .content()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|error| CoreError::repository("resource.encode_content", error))?;

        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|error| CoreError::repository("save_if_unchanged.begin", error))?;
        let result = sqlx::query(
            r#"
            UPDATE resources SET
                name = ?, directory = ?, kind = ?, status = ?, description = ?, content_json = ?,
                created_at = ?, updated_at = ?, deleted_at = ?
            WHERE id = ? AND updated_at = ?
            "#,
        )
        .bind(resource.name())
        .bind(resource.directory().path())
        .bind(resource.kind().as_str())
        .bind(resource.status().as_str())
        .bind(resource.description())
        .bind(content_json)
        .bind(encode_timestamp(resource.created_at()))
        .bind(encode_timestamp(resource.updated_at()))
        .bind(resource.deleted_at().map(encode_timestamp))
        .bind(resource.id().to_string())
        .bind(encode_timestamp(expected_updated_at))
        .execute(&mut *transaction)
        .await
        .map_err(|error| CoreError::repository("save_if_unchanged", error))?;

        if result.rows_affected() == 0 {
            transaction
                .rollback()
                .await
                .map_err(|error| CoreError::repository("save_if_unchanged.rollback", error))?;
            return Ok(false);
        }

        sync_tags(&mut transaction, resource).await?;
        transaction
            .commit()
            .await
            .map_err(|error| CoreError::repository("save_if_unchanged.commit", error))?;
        Ok(true)
    }

    async fn find_by_id(&self, id: &ResourceId) -> Result<Option<Resource>, CoreError> {
        let statement = format!("{RESOURCE_SELECT} WHERE resources.id = ?");
        let row = sqlx::query(&statement)
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| CoreError::repository("find_by_id", error))?;

        row.map(decode_resource).transpose()
    }

    async fn remove_if_unchanged(
        &self,
        id: &ResourceId,
        expected_updated_at: DateTime<Utc>,
    ) -> Result<bool, CoreError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|error| CoreError::repository("remove_if_unchanged.begin", error))?;
        let result = sqlx::query("DELETE FROM resources WHERE id = ? AND updated_at = ?")
            .bind(id.to_string())
            .bind(encode_timestamp(expected_updated_at))
            .execute(&mut *transaction)
            .await
            .map_err(|error| CoreError::repository("remove_if_unchanged", error))?;

        if result.rows_affected() == 1 {
            delete_orphan_tags(&mut transaction).await?;
        }
        transaction
            .commit()
            .await
            .map_err(|error| CoreError::repository("remove_if_unchanged.commit", error))?;

        Ok(result.rows_affected() == 1)
    }

    async fn remove(&self, id: &ResourceId) -> Result<(), CoreError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|error| CoreError::repository("remove.begin", error))?;
        sqlx::query("DELETE FROM resources WHERE id = ?")
            .bind(id.to_string())
            .execute(&mut *transaction)
            .await
            .map_err(|error| CoreError::repository("remove", error))?;
        delete_orphan_tags(&mut transaction).await?;
        transaction
            .commit()
            .await
            .map_err(|error| CoreError::repository("remove.commit", error))?;

        Ok(())
    }

    async fn save_directory(&self, directory: &ResourceDirectory) -> Result<(), CoreError> {
        ensure_directory_path(&self.pool, directory.parent_path()).await?;
        let now = encode_timestamp(Utc::now());
        sqlx::query("INSERT INTO directories (path, parent_path, name, created_at, updated_at) VALUES (?, ?, ?, ?, ?)")
            .bind(directory.path())
            .bind(directory.parent_path())
            .bind(directory.name())
            .bind(&now)
            .bind(&now)
            .execute(&self.pool)
            .await
            .map_err(|error| {
                if error.to_string().contains("UNIQUE") {
                    CoreError::conflict(format!("directory `{}` already exists", directory.path()))
                } else {
                    CoreError::repository("directory.create", error)
                }
            })?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl ResourceQuery for SqliteResourceRepository {
    async fn find_by_path(
        &self,
        directory: &ResourceDirectory,
        name: &str,
    ) -> Result<Option<Resource>, CoreError> {
        let statement =
            format!("{RESOURCE_SELECT} WHERE resources.directory = ? AND resources.name = ?");
        let row = sqlx::query(&statement)
            .bind(directory.path())
            .bind(name)
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| CoreError::repository("find_by_path", error))?;

        row.map(decode_resource).transpose()
    }

    async fn list(&self, query: &ListResources) -> Result<ResourcePage, CoreError> {
        let total: i64 = build_list_count_query(query)
            .build_query_scalar()
            .fetch_one(&self.pool)
            .await
            .map_err(|error| CoreError::repository("list.count", error))?;
        let rows = build_list_select_query(query)
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|error| CoreError::repository("list.select", error))?;
        let items = rows
            .into_iter()
            .map(decode_resource)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(ResourcePage {
            items,
            total: total as u64,
            limit: query.limit(),
            offset: query.offset(),
        })
    }

    async fn list_directories(
        &self,
        parent: &ResourceDirectory,
    ) -> Result<Vec<ResourceDirectory>, CoreError> {
        let rows = sqlx::query(
            r#"
            SELECT path, parent_path, name
            FROM directories
            WHERE parent_path = ?
            ORDER BY name ASC
            "#,
        )
        .bind(parent.path())
        .fetch_all(&self.pool)
        .await
        .map_err(|error| CoreError::repository("list_directories", error))?;

        rows.into_iter()
            .map(|row| {
                ResourceDirectory::rehydrate(
                    column(&row, "path")?,
                    column(&row, "parent_path")?,
                    column(&row, "name")?,
                )
                .map_err(CoreError::from)
            })
            .collect()
    }
}

fn build_list_count_query<'a>(query: &'a ListResources) -> QueryBuilder<'a, Sqlite> {
    let mut builder = QueryBuilder::new("SELECT COUNT(*) FROM resources");
    push_list_where(&mut builder, query);
    builder
}

fn build_list_select_query<'a>(query: &'a ListResources) -> QueryBuilder<'a, Sqlite> {
    let mut builder = QueryBuilder::new(RESOURCE_SELECT);
    push_list_where(&mut builder, query);
    builder.push(" ORDER BY resources.updated_at DESC, resources.id DESC LIMIT ");
    builder.push_bind(i64::from(query.limit()));
    builder.push(" OFFSET ");
    builder.push_bind(query.offset() as i64);
    builder
}

fn push_list_where<'a>(builder: &mut QueryBuilder<'a, Sqlite>, query: &'a ListResources) {
    let mut has_where = false;

    if !query.include_deleted() {
        push_condition_prefix(builder, &mut has_where);
        builder.push("resources.deleted_at IS NULL");
    }

    if !query.kinds().is_empty() {
        push_condition_prefix(builder, &mut has_where);
        builder.push("resources.kind IN (");
        let mut separated = builder.separated(", ");
        for kind in query.kinds() {
            separated.push_bind(kind.as_str());
        }
        separated.push_unseparated(")");
    }

    if let Some(tag) = query.tag() {
        push_condition_prefix(builder, &mut has_where);
        builder.push(
            "EXISTS (SELECT 1 FROM resource_tags JOIN tags ON tags.id = resource_tags.tag_id \
             WHERE resource_tags.resource_id = resources.id AND tags.name = ",
        );
        builder.push_bind(tag);
        builder.push(")");
    }

    if let Some(q) = query.q() {
        push_condition_prefix(builder, &mut has_where);
        builder.push("resources.name LIKE ");
        builder.push_bind(format!("%{}%", escape_like(q)));
        builder.push(" ESCAPE '\\'");
    }

    if let Some(directory) = query.directory() {
        push_condition_prefix(builder, &mut has_where);
        builder.push("resources.directory = ");
        builder.push_bind(directory.path());
    }
}

fn push_condition_prefix(builder: &mut QueryBuilder<Sqlite>, has_where: &mut bool) {
    if *has_where {
        builder.push(" AND ");
    } else {
        builder.push(" WHERE ");
        *has_where = true;
    }
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn decode_resource(row: SqliteRow) -> Result<Resource, CoreError> {
    let id = decode_id(column(&row, "id")?)?;
    let name = column(&row, "name")?;
    let directory = ResourceDirectory::from_path(column::<String>(&row, "directory")?)?;
    let kind = ResourceKind::try_new(column::<String>(&row, "kind")?)?;
    let status = status_from_str(column(&row, "status")?)?;
    let description = column(&row, "description")?;
    let tags = decode_tags(&row)?;
    let content = decode_content(column(&row, "content_json")?)?;
    let created_at = decode_timestamp(column(&row, "created_at")?)?;
    let updated_at = decode_timestamp(column(&row, "updated_at")?)?;
    let deleted_at = column::<Option<String>>(&row, "deleted_at")?
        .map(decode_timestamp)
        .transpose()?;

    Resource::rehydrate(ResourceSnapshot {
        id,
        name,
        directory,
        kind,
        status,
        description,
        tags,
        content,
        created_at,
        updated_at,
        deleted_at,
    })
    .map_err(CoreError::from)
}

async fn sync_tags(
    transaction: &mut Transaction<'_, Sqlite>,
    resource: &Resource,
) -> Result<(), CoreError> {
    let resource_id = resource.id().to_string();
    sqlx::query("DELETE FROM resource_tags WHERE resource_id = ?")
        .bind(&resource_id)
        .execute(&mut **transaction)
        .await
        .map_err(|error| CoreError::repository("resource.clear_tags", error))?;

    for tag in resource.tags() {
        sqlx::query("INSERT INTO tags (name) VALUES (?) ON CONFLICT(name) DO NOTHING")
            .bind(tag.as_str())
            .execute(&mut **transaction)
            .await
            .map_err(|error| CoreError::repository("resource.ensure_tag", error))?;
        sqlx::query(
            r#"
            INSERT INTO resource_tags (resource_id, tag_id)
            SELECT ?, id FROM tags WHERE name = ?
            "#,
        )
        .bind(&resource_id)
        .bind(tag.as_str())
        .execute(&mut **transaction)
        .await
        .map_err(|error| CoreError::repository("resource.save_tag", error))?;
    }

    delete_orphan_tags(transaction).await?;

    Ok(())
}

async fn delete_orphan_tags(transaction: &mut Transaction<'_, Sqlite>) -> Result<(), CoreError> {
    sqlx::query(
        r#"
        DELETE FROM tags
        WHERE NOT EXISTS (
            SELECT 1
            FROM resource_tags
            WHERE resource_tags.tag_id = tags.id
        )
        "#,
    )
    .execute(&mut **transaction)
    .await
    .map_err(|error| CoreError::repository("resource.delete_orphan_tags", error))?;

    Ok(())
}

async fn ensure_directory_path(pool: &SqlitePool, directory: &str) -> Result<(), CoreError> {
    if directory.is_empty() {
        return Ok(());
    }

    let now = encode_timestamp(Utc::now());
    let mut parent_path = String::new();
    for name in directory.split('/') {
        let path = if parent_path.is_empty() {
            name.to_string()
        } else {
            format!("{parent_path}/{name}")
        };

        sqlx::query(
            r#"
            INSERT INTO directories (path, parent_path, name, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(path) DO UPDATE SET
                parent_path = excluded.parent_path,
                name = excluded.name,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(&path)
        .bind(&parent_path)
        .bind(name)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await
        .map_err(|error| CoreError::repository("directory.save", error))?;

        parent_path = path;
    }

    Ok(())
}

fn column<T>(row: &SqliteRow, name: &'static str) -> Result<T, CoreError>
where
    for<'row> T: sqlx::Decode<'row, Sqlite> + sqlx::Type<Sqlite>,
{
    row.try_get(name)
        .map_err(|error| CoreError::repository("resource.decode_row", error))
}

fn decode_id(value: String) -> Result<ResourceId, CoreError> {
    value
        .parse()
        .map_err(|error| CoreError::repository("resource.decode_id", error))
}

fn decode_tags(row: &SqliteRow) -> Result<Vec<String>, CoreError> {
    let tags_json: String = column(row, "tags_json")?;
    serde_json::from_str::<Vec<String>>(&tags_json)
        .map_err(|error| CoreError::repository("resource.decode_tags", error))
}

fn decode_content(value: Option<String>) -> Result<Option<ResourceContent>, CoreError> {
    value
        .map(|value| {
            serde_json::from_str::<ResourceContent>(&value)
                .map_err(|error| CoreError::repository("resource.decode_content", error))
        })
        .transpose()
}

fn encode_timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339()
}

fn decode_timestamp(value: String) -> Result<DateTime<Utc>, CoreError> {
    DateTime::parse_from_rfc3339(&value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| CoreError::repository("resource.decode_timestamp", error))
}

fn status_from_str(value: String) -> Result<ResourceStatus, CoreError> {
    value
        .parse()
        .map_err(|error| CoreError::repository("resource.decode_status", error))
}

#[cfg(test)]
mod tests;
