use crate::migration;
use asset_core::CoreError;
use asset_core::domain::{
    Directory, DirectoryId, DirectoryKind, DirectoryPath, DirectorySnapshot, Resource,
    ResourceContent, ResourceId, ResourceKind, ResourceSnapshot,
};
use asset_core::port::{
    DirectoryLocation, DirectoryStore, ListResources, LocatedResource, ResourcePage, ResourceQuery,
    ResourceRepository,
};
use chrono::{DateTime, Utc};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow};
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool, Transaction};
use std::path::Path;

const RESOURCE_SELECT: &str = r#"
    WITH RECURSIVE directory_paths(id, parent_id, name, path) AS (
        SELECT id, parent_id, name, ''
        FROM directories
        WHERE id = '00000000-0000-0000-0000-000000000000'
        UNION ALL
        SELECT child.id, child.parent_id, child.name,
               CASE
                   WHEN parent.path = '' THEN child.name
                   ELSE parent.path || '/' || child.name
               END
        FROM directories child
        JOIN directory_paths parent ON child.parent_id = parent.id
    )
    SELECT
        resources.id,
        resources.name,
        resources.directory_id,
        directory_paths.path AS directory_path,
        resources.kind,
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
        resources.revision,
        resources.deleted_at
    FROM resources
    JOIN directory_paths ON directory_paths.id = resources.directory_id
"#;

const RESOURCE_AGGREGATE_SELECT: &str = r#"
    SELECT
        resources.id,
        resources.name,
        resources.directory_id,
        resources.kind,
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
        resources.revision,
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
    pub async fn connect(sqlite_path: &Path, max_connections: u32) -> Result<Self, CoreError> {
        if max_connections == 0 {
            return Err(CoreError::configuration(
                "database.sqlite.max_connections must be greater than 0",
            ));
        }

        if let Some(parent) = sqlite_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| CoreError::repository("sqlite.create_dir", error))?;
        }

        let options = SqliteConnectOptions::new()
            .filename(sqlite_path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal);

        let pool = SqlitePoolOptions::new()
            .max_connections(max_connections)
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
                directory_id,
                kind,
                content_json,
                created_at,
                updated_at,
                revision,
                deleted_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                directory_id = excluded.directory_id,
                kind = excluded.kind,
                content_json = excluded.content_json,
                created_at = excluded.created_at,
                updated_at = excluded.updated_at,
                revision = excluded.revision,
                deleted_at = excluded.deleted_at
            "#,
        )
        .bind(resource.id().to_string())
        .bind(resource.name())
        .bind(resource.directory_id().to_string())
        .bind(resource.kind().as_str())
        .bind(content_json)
        .bind(encode_timestamp(resource.created_at()))
        .bind(encode_timestamp(resource.updated_at()))
        .bind(encode_revision(resource.revision())?)
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
        expected_revision: u64,
    ) -> Result<bool, CoreError> {
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
                name = ?, directory_id = ?, kind = ?, content_json = ?,
                created_at = ?, updated_at = ?, revision = ?, deleted_at = ?
            WHERE id = ? AND revision = ?
            "#,
        )
        .bind(resource.name())
        .bind(resource.directory_id().to_string())
        .bind(resource.kind().as_str())
        .bind(content_json)
        .bind(encode_timestamp(resource.created_at()))
        .bind(encode_timestamp(resource.updated_at()))
        .bind(encode_revision(resource.revision())?)
        .bind(resource.deleted_at().map(encode_timestamp))
        .bind(resource.id().to_string())
        .bind(encode_revision(expected_revision)?)
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
        let statement = format!("{RESOURCE_AGGREGATE_SELECT} WHERE resources.id = ?");
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
        expected_revision: u64,
    ) -> Result<bool, CoreError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|error| CoreError::repository("remove_if_unchanged.begin", error))?;
        let result = sqlx::query("DELETE FROM resources WHERE id = ? AND revision = ?")
            .bind(id.to_string())
            .bind(encode_revision(expected_revision)?)
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
}

#[async_trait::async_trait]
impl ResourceQuery for SqliteResourceRepository {
    async fn find_located_by_id(
        &self,
        id: &ResourceId,
    ) -> Result<Option<LocatedResource>, CoreError> {
        let statement = format!("{RESOURCE_SELECT} WHERE resources.id = ?");
        let row = sqlx::query(&statement)
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| CoreError::repository("query.find_by_id", error))?;

        row.map(decode_located_resource).transpose()
    }

    async fn find_by_path(
        &self,
        directory: &DirectoryPath,
        name: &str,
    ) -> Result<Option<LocatedResource>, CoreError> {
        let statement = format!(
            "{RESOURCE_SELECT} WHERE directory_paths.path = ? AND resources.name = ? \
             AND resources.deleted_at IS NULL"
        );
        let row = sqlx::query(&statement)
            .bind(directory.path())
            .bind(name)
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| CoreError::repository("find_by_path", error))?;

        row.map(decode_located_resource).transpose()
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
            .map(decode_located_resource)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(ResourcePage {
            items,
            total: total as u64,
            limit: query.limit(),
            offset: query.offset(),
        })
    }
}

#[async_trait::async_trait]
impl DirectoryStore for SqliteResourceRepository {
    async fn load_all(&self) -> Result<Vec<Directory>, CoreError> {
        let rows = sqlx::query(
            "SELECT id, parent_id, name, kind, created_at, updated_at, revision FROM directories",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|error| CoreError::repository("directory.load_all", error))?;
        rows.into_iter().map(decode_directory).collect()
    }

    async fn insert(&self, directory: &Directory) -> Result<(), CoreError> {
        sqlx::query(
            r#"
            INSERT INTO directories (
                id, parent_id, name, kind, created_at, updated_at, revision
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(directory.id().to_string())
        .bind(directory.parent_id().map(|id| id.to_string()))
        .bind(directory.name())
        .bind(directory.kind().as_str())
        .bind(encode_timestamp(directory.created_at()))
        .bind(encode_timestamp(directory.updated_at()))
        .bind(encode_directory_revision(directory.revision())?)
        .execute(&self.pool)
        .await
        .map_err(map_directory_write_error("directory.insert"))?;
        Ok(())
    }

    async fn save_if_unchanged(
        &self,
        directory: &Directory,
        expected_revision: u64,
    ) -> Result<bool, CoreError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|error| CoreError::repository("directory.save.begin", error))?;
        if let Some(parent_id) = directory.parent_id() {
            let creates_cycle = sqlx::query_scalar::<_, i64>(
                r#"
                WITH RECURSIVE subtree(id) AS (
                    SELECT id FROM directories WHERE id = ?
                    UNION ALL
                    SELECT child.id
                    FROM directories child
                    JOIN subtree parent ON child.parent_id = parent.id
                )
                SELECT EXISTS(SELECT 1 FROM subtree WHERE id = ?)
                "#,
            )
            .bind(directory.id().to_string())
            .bind(parent_id.to_string())
            .fetch_one(&mut *transaction)
            .await
            .map_err(|error| CoreError::repository("directory.save.check_cycle", error))?;
            if creates_cycle != 0 {
                transaction
                    .rollback()
                    .await
                    .map_err(|error| CoreError::repository("directory.save.rollback", error))?;
                return Err(CoreError::conflict(
                    "moving the directory would create a cycle",
                ));
            }
        }
        let result = sqlx::query(
            r#"
            UPDATE directories
            SET parent_id = ?, name = ?, kind = ?, updated_at = ?, revision = ?
            WHERE id = ? AND revision = ?
            "#,
        )
        .bind(directory.parent_id().map(|id| id.to_string()))
        .bind(directory.name())
        .bind(directory.kind().as_str())
        .bind(encode_timestamp(directory.updated_at()))
        .bind(encode_directory_revision(directory.revision())?)
        .bind(directory.id().to_string())
        .bind(encode_directory_revision(expected_revision)?)
        .execute(&mut *transaction)
        .await
        .map_err(map_directory_write_error("directory.save_if_unchanged"))?;
        transaction
            .commit()
            .await
            .map_err(|error| CoreError::repository("directory.save.commit", error))?;
        Ok(result.rows_affected() == 1)
    }

    async fn remove_if_empty(&self, id: &DirectoryId) -> Result<bool, CoreError> {
        if id.is_root() {
            return Ok(false);
        }
        let result = sqlx::query(
            r#"
            DELETE FROM directories
            WHERE id = ?
              AND NOT EXISTS (SELECT 1 FROM directories child WHERE child.parent_id = directories.id)
              AND NOT EXISTS (SELECT 1 FROM resources WHERE resources.directory_id = directories.id)
            "#,
        )
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|error| CoreError::repository("directory.remove_if_empty", error))?;
        Ok(result.rows_affected() == 1)
    }
}

fn map_directory_write_error(operation: &'static str) -> impl FnOnce(sqlx::Error) -> CoreError {
    move |error| {
        if error.to_string().contains("UNIQUE") {
            CoreError::conflict("a directory with the same name already exists")
        } else {
            CoreError::repository(operation, error)
        }
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

    if let Some(directory_id) = query.directory_id() {
        push_condition_prefix(builder, &mut has_where);
        builder.push("resources.directory_id = ");
        builder.push_bind(directory_id.to_string());
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
    let directory_id = decode_directory_id(column(&row, "directory_id")?)?;
    let kind = ResourceKind::try_new(column::<String>(&row, "kind")?)
        .map_err(|error| CoreError::repository("resource.decode_kind", error))?;
    let tags = decode_tags(&row)?;
    let content = decode_content(column(&row, "content_json")?)?;
    let created_at = decode_timestamp(column(&row, "created_at")?)?;
    let updated_at = decode_timestamp(column(&row, "updated_at")?)?;
    let revision = column::<i64>(&row, "revision")?;
    let revision = u64::try_from(revision)
        .map_err(|error| CoreError::repository("resource.decode_revision", error))?;
    let deleted_at = column::<Option<String>>(&row, "deleted_at")?
        .map(decode_timestamp)
        .transpose()?;

    Resource::rehydrate(ResourceSnapshot {
        id,
        name,
        directory_id,
        kind,
        tags,
        content,
        created_at,
        updated_at,
        revision,
        deleted_at,
    })
    .map_err(|error| CoreError::repository("resource.rehydrate", error))
}

fn decode_located_resource(row: SqliteRow) -> Result<LocatedResource, CoreError> {
    let directory_id = decode_directory_id(column(&row, "directory_id")?)?;
    let directory_path = DirectoryPath::from_path(column::<String>(&row, "directory_path")?)
        .map_err(|error| CoreError::repository("resource.decode_directory_path", error))?;
    let resource = decode_resource(row)?;
    LocatedResource::new(
        resource,
        DirectoryLocation::new(directory_id, directory_path),
    )
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

fn decode_directory_id(value: String) -> Result<DirectoryId, CoreError> {
    value
        .parse()
        .map_err(|error| CoreError::repository("directory.decode_id", error))
}

fn decode_directory(row: SqliteRow) -> Result<Directory, CoreError> {
    Directory::rehydrate(DirectorySnapshot {
        id: decode_directory_id(column(&row, "id")?)?,
        parent_id: column::<Option<String>>(&row, "parent_id")?
            .map(decode_directory_id)
            .transpose()?,
        name: column(&row, "name")?,
        kind: DirectoryKind::try_new(column::<String>(&row, "kind")?)
            .map_err(|error| CoreError::repository("directory.decode_kind", error))?,
        created_at: decode_timestamp(column(&row, "created_at")?)?,
        updated_at: decode_timestamp(column(&row, "updated_at")?)?,
        revision: decode_revision(column(&row, "revision")?)?,
    })
    .map_err(|error| CoreError::repository("directory.rehydrate", error))
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

fn encode_revision(value: u64) -> Result<i64, CoreError> {
    i64::try_from(value).map_err(|error| CoreError::repository("resource.encode_revision", error))
}

fn decode_revision(value: i64) -> Result<u64, CoreError> {
    u64::try_from(value).map_err(|error| CoreError::repository("directory.decode_revision", error))
}

fn encode_directory_revision(value: u64) -> Result<i64, CoreError> {
    i64::try_from(value).map_err(|error| CoreError::repository("directory.encode_revision", error))
}

fn decode_timestamp(value: String) -> Result<DateTime<Utc>, CoreError> {
    DateTime::parse_from_rfc3339(&value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| CoreError::repository("resource.decode_timestamp", error))
}

#[cfg(test)]
mod tests;
