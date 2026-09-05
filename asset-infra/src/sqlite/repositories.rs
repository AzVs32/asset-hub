use crate::migration;
use asset_core::CoreError;
use asset_core::domain::{
    Directory, DirectoryId, DirectoryPath, Resource, ResourceContent, ResourceId,
};
use asset_core::port::{
    DirectoryLocation, DirectoryRelocation, DirectoryRelocationStore, DirectoryRevisionUpdate,
    DirectoryStore, ListResources, LocatedResource, ResourceMaintenanceReadModel, ResourcePage,
    ResourceReadModel, ResourceRelocation, ResourceRelocationStore, ResourceStore,
};
use chrono::{DateTime, Utc};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{QueryBuilder, Sqlite, SqlitePool};
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
        resources.content_json,
        resources.created_at,
        resources.updated_at,
        resources.revision
    FROM resources
    JOIN directory_paths ON directory_paths.id = resources.directory_id
"#;

const RESOURCE_AGGREGATE_SELECT: &str = r#"
    SELECT
        resources.id,
        resources.name,
        resources.directory_id,
        resources.content_json,
        resources.created_at,
        resources.updated_at,
        resources.revision
    FROM resources
"#;

/// SQLite 资源记录；领域值解析和聚合校验在读取后显式执行。
#[derive(sqlx::FromRow)]
struct ResourceRow {
    id: String,
    name: String,
    directory_id: String,
    content_json: Option<String>,
    created_at: String,
    updated_at: String,
    revision: i64,
}

#[derive(sqlx::FromRow)]
struct LocatedResourceRow {
    #[sqlx(flatten)]
    resource: ResourceRow,
    directory_path: String,
}

#[derive(sqlx::FromRow)]
struct ResourceRelocationRow {
    resource_id: String,
    expected_revision: i64,
    source_key: String,
    destination_key: String,
    name: String,
    directory_id: String,
    content_json: Option<String>,
    created_at: String,
    updated_at: String,
    revision: i64,
}

/// SQLite 目录记录，与 Core 的目录聚合保持解耦。
#[derive(sqlx::FromRow)]
struct DirectoryRow {
    id: String,
    parent_id: Option<String>,
    name: String,
    created_at: String,
    updated_at: String,
    revision: i64,
}

#[derive(Clone)]
pub(crate) struct SqliteDatabase {
    pool: SqlitePool,
}

impl SqliteDatabase {
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

        let database = Self { pool };
        database.run_migrations().await?;

        Ok(database)
    }

    /// 使用已有连接池创建仓储。
    ///
    /// 该构造函数不会自动执行迁移，主要用于测试或由外部迁移系统管理 schema 的场景。
    pub(crate) fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// 执行 SQLite 数据库迁移。
    pub async fn run_migrations(&self) -> Result<(), CoreError> {
        migration::sqlite::run(&self.pool).await
    }
}

/// SQLite adapter for the Resource aggregate and its query projection.
#[derive(Clone)]
pub struct SqliteResourceStore {
    pool: SqlitePool,
}

impl SqliteResourceStore {
    pub(crate) fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn map_resource_insert_error(error: sqlx::Error) -> CoreError {
    if error.to_string().contains("UNIQUE") {
        CoreError::conflict("a resource with the same identity or directory name already exists")
    } else {
        CoreError::repository("resource.insert", error)
    }
}

/// SQLite adapter for the Directory aggregate.
#[derive(Clone)]
pub struct SqliteDirectoryStore {
    pool: SqlitePool,
}

impl SqliteDirectoryStore {
    pub(crate) fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl ResourceStore for SqliteResourceStore {
    async fn health_check(&self) -> Result<(), CoreError> {
        sqlx::query_scalar::<_, i64>("SELECT 1")
            .fetch_one(&self.pool)
            .await
            .map(|_| ())
            .map_err(|error| CoreError::repository("health_check", error))
    }

    async fn insert(&self, resource: &Resource) -> Result<(), CoreError> {
        let content_json = resource
            .content()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|error| CoreError::repository("resource.encode_content", error))?;

        sqlx::query(
            r#"
            INSERT INTO resources (
                id,
                name,
                directory_id,
                content_json,
                created_at,
                updated_at,
                revision
            )
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(resource.id().to_string())
        .bind(resource.name())
        .bind(resource.directory_id().to_string())
        .bind(content_json)
        .bind(encode_timestamp(resource.created_at()))
        .bind(encode_timestamp(resource.updated_at()))
        .bind(encode_revision(resource.revision())?)
        .execute(&self.pool)
        .await
        .map_err(map_resource_insert_error)?;

        Ok(())
    }

    async fn update_if_revision(
        &self,
        resource: &Resource,
        expected_revision: u64,
    ) -> Result<bool, CoreError> {
        let content_json = resource
            .content()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|error| CoreError::repository("resource.encode_content", error))?;

        let result = sqlx::query(
            r#"
            UPDATE resources SET
                name = ?, directory_id = ?, content_json = ?,
                created_at = ?, updated_at = ?, revision = ?
            WHERE id = ? AND revision = ?
            "#,
        )
        .bind(resource.name())
        .bind(resource.directory_id().to_string())
        .bind(content_json)
        .bind(encode_timestamp(resource.created_at()))
        .bind(encode_timestamp(resource.updated_at()))
        .bind(encode_revision(resource.revision())?)
        .bind(resource.id().to_string())
        .bind(encode_revision(expected_revision)?)
        .execute(&self.pool)
        .await
        .map_err(|error| CoreError::repository("resource.update_if_revision", error))?;
        Ok(result.rows_affected() == 1)
    }

    async fn load(&self, id: &ResourceId) -> Result<Option<Resource>, CoreError> {
        let statement = format!("{RESOURCE_AGGREGATE_SELECT} WHERE resources.id = ?");
        let row = sqlx::query_as::<_, ResourceRow>(&statement)
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| CoreError::repository("find_by_id", error))?;

        row.map(decode_resource).transpose()
    }

    async fn delete_if_revision(
        &self,
        id: &ResourceId,
        expected_revision: u64,
    ) -> Result<bool, CoreError> {
        let result = sqlx::query("DELETE FROM resources WHERE id = ? AND revision = ?")
            .bind(id.to_string())
            .bind(encode_revision(expected_revision)?)
            .execute(&self.pool)
            .await
            .map_err(|error| CoreError::repository("resource.delete_if_revision", error))?;

        Ok(result.rows_affected() == 1)
    }
}

#[async_trait::async_trait]
impl ResourceReadModel for SqliteResourceStore {
    async fn find_by_id(&self, id: &ResourceId) -> Result<Option<LocatedResource>, CoreError> {
        let statement = format!("{RESOURCE_SELECT} WHERE resources.id = ?");
        let row = sqlx::query_as::<_, LocatedResourceRow>(&statement)
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| CoreError::repository("query.find_by_id", error))?;

        row.map(decode_located_resource).transpose()
    }

    async fn find_by_directory_and_name(
        &self,
        directory_id: DirectoryId,
        name: &str,
    ) -> Result<Option<LocatedResource>, CoreError> {
        let statement =
            format!("{RESOURCE_SELECT} WHERE resources.directory_id = ? AND resources.name = ?");
        let row = sqlx::query_as::<_, LocatedResourceRow>(&statement)
            .bind(directory_id.to_string())
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
            .build_query_as::<LocatedResourceRow>()
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
impl ResourceMaintenanceReadModel for SqliteResourceStore {
    async fn list_all(&self) -> Result<Vec<LocatedResource>, CoreError> {
        sqlx::query_as::<_, LocatedResourceRow>(RESOURCE_SELECT)
            .fetch_all(&self.pool)
            .await
            .map_err(|error| CoreError::repository("resource.maintenance.list_all", error))?
            .into_iter()
            .map(decode_located_resource)
            .collect()
    }
}

#[async_trait::async_trait]
impl ResourceRelocationStore for SqliteResourceStore {
    async fn save(&self, relocation: &ResourceRelocation) -> Result<(), CoreError> {
        let desired = relocation.desired();
        let content_json = desired
            .content()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|error| CoreError::repository("resource.relocation.encode_content", error))?;
        sqlx::query(
            r#"
            INSERT INTO resource_relocations (
                resource_id, expected_revision, source_key, destination_key,
                name, directory_id, content_json, created_at, updated_at, revision
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(desired.id().to_string())
        .bind(encode_revision(relocation.expected_revision())?)
        .bind(relocation.source_key().as_str())
        .bind(relocation.destination_key().as_str())
        .bind(desired.name())
        .bind(desired.directory_id().to_string())
        .bind(content_json)
        .bind(encode_timestamp(desired.created_at()))
        .bind(encode_timestamp(desired.updated_at()))
        .bind(encode_revision(desired.revision())?)
        .execute(&self.pool)
        .await
        .map_err(|error| CoreError::repository("resource.relocation.save", error))?;
        Ok(())
    }

    async fn load_all(&self) -> Result<Vec<ResourceRelocation>, CoreError> {
        let rows = sqlx::query_as::<_, ResourceRelocationRow>(
            r#"
            SELECT resource_id, expected_revision, source_key, destination_key,
                   name, directory_id, content_json, created_at, updated_at, revision
            FROM resource_relocations
            ORDER BY resource_id
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|error| CoreError::repository("resource.relocation.load_all", error))?;

        rows.into_iter()
            .map(|row| {
                let desired = decode_resource(ResourceRow {
                    id: row.resource_id,
                    name: row.name,
                    directory_id: row.directory_id,
                    content_json: row.content_json,
                    created_at: row.created_at,
                    updated_at: row.updated_at,
                    revision: row.revision,
                })?;
                ResourceRelocation::new(
                    desired,
                    decode_revision(row.expected_revision)?,
                    asset_core::domain::StorageKey::new(row.source_key)?,
                    asset_core::domain::StorageKey::new(row.destination_key)?,
                )
            })
            .collect()
    }

    async fn complete(&self, id: &ResourceId) -> Result<(), CoreError> {
        sqlx::query("DELETE FROM resource_relocations WHERE resource_id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|error| CoreError::repository("resource.relocation.complete", error))?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl DirectoryStore for SqliteDirectoryStore {
    async fn load_all(&self) -> Result<Vec<Directory>, CoreError> {
        let rows = sqlx::query_as::<_, DirectoryRow>(
            "SELECT id, parent_id, name, created_at, updated_at, revision FROM directories",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|error| CoreError::repository("directory.load_all", error))?;
        rows.into_iter().map(decode_directory).collect()
    }

    async fn load(&self, id: &DirectoryId) -> Result<Option<Directory>, CoreError> {
        let row = sqlx::query_as::<_, DirectoryRow>(
            "SELECT id, parent_id, name, created_at, updated_at, revision FROM directories WHERE id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| CoreError::repository("directory.load", error))?;
        row.map(decode_directory).transpose()
    }

    async fn insert(&self, directory: &Directory) -> Result<(), CoreError> {
        sqlx::query(
            r#"
            INSERT INTO directories (
                id, parent_id, name, created_at, updated_at, revision
            ) VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(directory.id().to_string())
        .bind(directory.parent_id().map(|id| id.to_string()))
        .bind(directory.name())
        .bind(encode_timestamp(directory.created_at()))
        .bind(encode_timestamp(directory.updated_at()))
        .bind(encode_directory_revision(directory.revision())?)
        .execute(&self.pool)
        .await
        .map_err(map_directory_write_error("directory.insert"))?;
        Ok(())
    }

    async fn update_batch_if_unchanged(
        &self,
        updates: &[DirectoryRevisionUpdate],
    ) -> Result<bool, CoreError> {
        if updates.is_empty() {
            return Ok(true);
        }
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|error| CoreError::repository("directory.save.begin", error))?;
        for update in updates {
            let directory = update.directory();
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
                .map_err(|error| CoreError::repository("directory.update.check_cycle", error))?;
                if creates_cycle != 0 {
                    transaction.rollback().await.map_err(|error| {
                        CoreError::repository("directory.update.rollback", error)
                    })?;
                    return Err(CoreError::conflict(
                        "moving the directory would create a cycle",
                    ));
                }
            }
            let result = sqlx::query(
                r#"
                UPDATE directories
                SET parent_id = ?, name = ?, updated_at = ?, revision = ?
                WHERE id = ? AND revision = ?
                "#,
            )
            .bind(directory.parent_id().map(|id| id.to_string()))
            .bind(directory.name())
            .bind(encode_timestamp(directory.updated_at()))
            .bind(encode_directory_revision(directory.revision())?)
            .bind(directory.id().to_string())
            .bind(encode_directory_revision(update.expected_revision())?)
            .execute(&mut *transaction)
            .await
            .map_err(map_directory_write_error("directory.update_batch"))?;
            if result.rows_affected() != 1 {
                transaction
                    .rollback()
                    .await
                    .map_err(|error| CoreError::repository("directory.update.rollback", error))?;
                return Ok(false);
            }
        }
        transaction
            .commit()
            .await
            .map_err(|error| CoreError::repository("directory.save.commit", error))?;
        Ok(true)
    }

    async fn is_empty(&self, id: &DirectoryId) -> Result<bool, CoreError> {
        let empty = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT NOT EXISTS (SELECT 1 FROM directories WHERE parent_id = ?)
               AND NOT EXISTS (SELECT 1 FROM resources WHERE directory_id = ?)
            "#,
        )
        .bind(id.to_string())
        .bind(id.to_string())
        .fetch_one(&self.pool)
        .await
        .map_err(|error| CoreError::repository("directory.is_empty", error))?;
        Ok(empty != 0)
    }

    async fn delete_if_empty(
        &self,
        id: &DirectoryId,
        expected_revision: u64,
    ) -> Result<bool, CoreError> {
        if id.is_root() {
            return Ok(false);
        }
        let result = sqlx::query(
            r#"
            DELETE FROM directories
            WHERE id = ? AND revision = ?
              AND NOT EXISTS (SELECT 1 FROM directories child WHERE child.parent_id = directories.id)
              AND NOT EXISTS (SELECT 1 FROM resources WHERE resources.directory_id = directories.id)
            "#,
        )
        .bind(id.to_string())
        .bind(encode_directory_revision(expected_revision)?)
        .execute(&self.pool)
        .await
        .map_err(|error| CoreError::repository("directory.delete_if_empty", error))?;
        Ok(result.rows_affected() == 1)
    }
}

#[async_trait::async_trait]
impl DirectoryRelocationStore for SqliteDirectoryStore {
    async fn begin(&self, relocation: &DirectoryRelocation) -> Result<(), CoreError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|error| CoreError::repository("directory.relocation.begin", error))?;
        sqlx::query(
            "INSERT INTO directory_relocations (directory_id, source_path, destination_path) VALUES (?, ?, ?)",
        )
        .bind(relocation.directory_id().to_string())
        .bind(relocation.source().path())
        .bind(relocation.destination().path())
        .execute(&mut *transaction)
        .await
        .map_err(map_directory_write_error("directory.relocation.insert"))?;
        for (position, update) in relocation.updates().iter().enumerate() {
            let directory = update.directory();
            sqlx::query(
                r#"
                INSERT INTO directory_relocation_updates (
                    relocation_directory_id, position, directory_id, expected_revision,
                    parent_id, name, created_at, updated_at, revision
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(relocation.directory_id().to_string())
            .bind(
                i64::try_from(position).map_err(|_| {
                    CoreError::invariant("directory relocation has too many updates")
                })?,
            )
            .bind(directory.id().to_string())
            .bind(encode_directory_revision(update.expected_revision())?)
            .bind(directory.parent_id().map(|id| id.to_string()))
            .bind(directory.name())
            .bind(encode_timestamp(directory.created_at()))
            .bind(encode_timestamp(directory.updated_at()))
            .bind(encode_directory_revision(directory.revision())?)
            .execute(&mut *transaction)
            .await
            .map_err(map_directory_write_error(
                "directory.relocation.insert_update",
            ))?;
        }
        transaction
            .commit()
            .await
            .map_err(|error| CoreError::repository("directory.relocation.commit", error))
    }

    async fn load_pending(&self) -> Result<Vec<DirectoryRelocation>, CoreError> {
        #[derive(sqlx::FromRow)]
        struct RelocationRow {
            directory_id: String,
            source_path: String,
            destination_path: String,
        }
        #[derive(sqlx::FromRow)]
        struct UpdateRow {
            directory_id: String,
            expected_revision: i64,
            parent_id: Option<String>,
            name: String,
            created_at: String,
            updated_at: String,
            revision: i64,
        }

        let rows = sqlx::query_as::<_, RelocationRow>(
            "SELECT directory_id, source_path, destination_path FROM directory_relocations ORDER BY directory_id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|error| CoreError::repository("directory.relocation.load", error))?;
        let mut relocations = Vec::with_capacity(rows.len());
        for row in rows {
            let update_rows = sqlx::query_as::<_, UpdateRow>(
                r#"
                SELECT directory_id, expected_revision, parent_id, name,
                       created_at, updated_at, revision
                FROM directory_relocation_updates
                WHERE relocation_directory_id = ?
                ORDER BY position
                "#,
            )
            .bind(&row.directory_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|error| CoreError::repository("directory.relocation.load_updates", error))?;
            let mut updates = Vec::with_capacity(update_rows.len());
            for update in update_rows {
                let directory = decode_directory(DirectoryRow {
                    id: update.directory_id,
                    parent_id: update.parent_id,
                    name: update.name,
                    created_at: update.created_at,
                    updated_at: update.updated_at,
                    revision: update.revision,
                })?;
                updates.push(DirectoryRevisionUpdate::new(
                    directory,
                    decode_revision(update.expected_revision)?,
                )?);
            }
            relocations.push(DirectoryRelocation::new(
                decode_directory_id(&row.directory_id)?,
                DirectoryPath::from_path(row.source_path)?,
                DirectoryPath::from_path(row.destination_path)?,
                updates,
            )?);
        }
        Ok(relocations)
    }

    async fn complete(&self, directory_id: &DirectoryId) -> Result<(), CoreError> {
        sqlx::query("DELETE FROM directory_relocations WHERE directory_id = ?")
            .bind(directory_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|error| CoreError::repository("directory.relocation.complete", error))?;
        Ok(())
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

    if let Some(q) = query.q() {
        push_condition_prefix(builder, &mut has_where);
        builder.push("resources.name LIKE ");
        builder.push_bind(format!("%{}%", escape_like(q)));
        builder.push(" ESCAPE '\\'");
    }

    push_condition_prefix(builder, &mut has_where);
    builder.push("resources.directory_id = ");
    builder.push_bind(query.directory_id().to_string());
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

fn decode_resource(row: ResourceRow) -> Result<Resource, CoreError> {
    let ResourceRow {
        id,
        name,
        directory_id,
        content_json,
        created_at,
        updated_at,
        revision,
    } = row;
    let content = decode_content(content_json)?;
    let revision = u64::try_from(revision)
        .map_err(|error| CoreError::repository("resource.decode_revision", error))?;

    Resource::rehydrate(
        decode_id(&id)?,
        name,
        decode_directory_id(&directory_id)?,
        content,
        decode_timestamp("resource.decode_created_at", &created_at)?,
        decode_timestamp("resource.decode_updated_at", &updated_at)?,
        revision,
    )
    .map_err(|error| CoreError::repository("resource.rehydrate", error))
}

fn decode_located_resource(row: LocatedResourceRow) -> Result<LocatedResource, CoreError> {
    let directory_id = decode_directory_id(&row.resource.directory_id)?;
    let directory_path = DirectoryPath::from_path(row.directory_path)
        .map_err(|error| CoreError::repository("resource.decode_directory_path", error))?;
    let resource = decode_resource(row.resource)?;
    LocatedResource::new(
        resource,
        DirectoryLocation::new(directory_id, directory_path),
    )
}

fn decode_id(value: &str) -> Result<ResourceId, CoreError> {
    value
        .parse()
        .map_err(|error| CoreError::repository("resource.decode_id", error))
}

fn decode_directory_id(value: &str) -> Result<DirectoryId, CoreError> {
    value
        .parse()
        .map_err(|error| CoreError::repository("directory.decode_id", error))
}

fn decode_directory(row: DirectoryRow) -> Result<Directory, CoreError> {
    Directory::rehydrate(
        decode_directory_id(&row.id)?,
        row.parent_id
            .as_deref()
            .map(decode_directory_id)
            .transpose()?,
        row.name,
        decode_timestamp("directory.decode_created_at", &row.created_at)?,
        decode_timestamp("directory.decode_updated_at", &row.updated_at)?,
        decode_revision(row.revision)?,
    )
    .map_err(|error| CoreError::repository("directory.rehydrate", error))
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

fn decode_timestamp(operation: &'static str, value: &str) -> Result<DateTime<Utc>, CoreError> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| CoreError::repository(operation, error))
}

#[cfg(test)]
mod tests;
