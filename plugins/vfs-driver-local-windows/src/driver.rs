//! Local-directory instances backed by an independently supplied SQLite pool.

use crate::{WindowsLocalDriverError, WindowsLocalStorage};
use ah_vfs::application::driver_info::DriverInfo;
use ah_vfs::application::dto::{
    InstanceConfig, InstanceConfigSchema, InstanceInfo, InstancePage, InstanceQuery,
    RemoveInstanceOutcome, instance_config_string, validate_instance_config_fields,
};
use ah_vfs::application::port::{Driver, DriverFuture, Storage};
use ah_vfs::domain::{
    ChildMountPolicy, DriverError, DriverKey, InstanceConfigErrorKind, InstanceId,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use sqlx::{Row, sqlite::SqliteRow};
use vfs_driver_kit::{InstanceConfigRow, InstanceStore, SqliteInsert, StoredInstance};

/// An instance's stable driver-owned identifier and canonical local directory.
struct LocalConfig {
    root: PathBuf,
}
type LocalInstance = StoredInstance<LocalConfig>;

/// Windows-only gRPC plugin driver with self-managed instance records.
/// Instance queries expose the canonical UTF-8 path as `details.root_path`.
pub struct WindowsLocalDriver {
    info: DriverInfo,
    registry: Arc<LocalRegistry>,
}

impl WindowsLocalDriver {
    /// The declaration key and `[vfs.driver]` child key share the same identity.
    pub const KEY: &str = "local";

    /// Initialize only WindowsLocalDriver's tables in the supplied driver database.
    ///
    /// The plugin owns the pool's connection settings and lifetime. Both file
    /// and memory pools are accepted; the application host never supplies it over RPC.
    pub async fn from_pool(pool: sqlx::SqlitePool) -> Result<Self, WindowsLocalDriverError> {
        let key = DriverKey::parse(Self::KEY).map_err(|_| WindowsLocalDriverError::InvalidData)?;
        Ok(Self {
            info: DriverInfo::builder()
                .maybe_instance_config_schema(instance_config_schema())
                .capabilities(WindowsLocalStorage::supported_capabilities())
                .key(key)
                .name("Local filesystem".into())
                .version(semver::Version::new(0, 1, 0))
                .child_mount_policy(ChildMountPolicy::Allow)
                .build(),
            registry: Arc::new(LocalRegistry {
                instances: InstanceStore::from_pool(pool)
                    .await
                    .map_err(WindowsLocalDriverError::Registry)?,
            }),
        })
    }
}

async fn canonical_dir(source: &Path) -> Result<PathBuf, WindowsLocalDriverError> {
    let root = tokio::fs::canonicalize(source)
        .await
        .map_err(|source_error| WindowsLocalDriverError::Dir {
            path: source.to_path_buf(),
            source: source_error,
        })?;
    let metadata = tokio::fs::metadata(&root)
        .await
        .map_err(|source| WindowsLocalDriverError::Dir {
            path: root.clone(),
            source,
        })?;
    if !metadata.is_dir() {
        return Err(WindowsLocalDriverError::NotDir(root));
    }
    Ok(root)
}

impl Driver for WindowsLocalDriver {
    fn instances(&self) -> ah_vfs::application::port::InstanceManagement {
        ah_vfs::application::port::InstanceManagement::SelfManaged(self.registry.clone())
    }

    fn info(&self) -> &DriverInfo {
        &self.info
    }

    fn open(
        &self,
        context: ah_vfs::application::port::OpenContext,
    ) -> DriverFuture<'_, Arc<dyn Storage>> {
        let instance_id = context.instance_id().clone();
        let registry = self.registry.clone();
        Box::pin(async move {
            let instance = registry
                .instances
                .get(&instance_id)
                .await?
                .ok_or(DriverError::InstanceNotFound(instance_id))?;
            let root = canonical_dir(&instance.config().root)
                .await
                .map_err(DriverError::from)?;
            if root != instance.config().root {
                return Err(DriverError::from(WindowsLocalDriverError::InvalidData));
            }
            let storage = WindowsLocalStorage::open(root).await.map_err(DriverError::from)?;
            Ok(Arc::new(storage) as Arc<dyn Storage>)
        })
    }
}

fn instance_info(instance: LocalInstance) -> Result<InstanceInfo, WindowsLocalDriverError> {
    let root = instance
        .config()
        .root
        .to_str()
        .ok_or(WindowsLocalDriverError::InvalidPathEncoding)?;
    Ok(InstanceInfo {
        id: instance.id().clone(),
        details: BTreeMap::from([("root_path".into(), root.into())]),
    })
}

struct LocalRegistry {
    instances: InstanceStore<LocalConfig>,
}

fn instance_config_schema() -> Option<InstanceConfigSchema> {
    Some(BTreeMap::from([
        ("type".into(), "object".into()),
        (
            "properties".into(),
            serde_json::json!({"root_path": {"type": "string", "minLength": 1, "description": "Existing UTF-8 directory; relative paths use the process working directory"}}),
        ),
        ("required".into(), serde_json::json!(["root_path"])),
        ("additionalProperties".into(), false.into()),
    ]))
}

impl ah_vfs::application::port::InstanceRegistry for LocalRegistry {
    fn create(&self, config: InstanceConfig) -> DriverFuture<'_, InstanceInfo> {
        Box::pin(async move {
            validate_instance_config_fields(&config, &["root_path"])?;
            let source = instance_config_string(&config, "root_path")?;
            if source.is_empty() || source.contains('\0') {
                return Err(DriverError::InvalidInstanceConfig {
                    field: "root_path".into(),
                    kind: InstanceConfigErrorKind::InvalidValue,
                });
            }
            let root = canonical_dir(Path::new(source))
                .await
                .map_err(DriverError::from)?;
            root.to_str().ok_or(WindowsLocalDriverError::InvalidPathEncoding)?;
            let record = self.instances.insert(LocalConfig { root }).await?;
            Ok(instance_info(record)?)
        })
    }
    fn get(&self, id: InstanceId) -> DriverFuture<'_, Option<InstanceInfo>> {
        Box::pin(async move {
            self.instances
                .get(&id)
                .await?
                .map(instance_info)
                .transpose()
                .map_err(DriverError::from)
        })
    }
    fn list(&self, query: InstanceQuery) -> DriverFuture<'_, InstancePage> {
        Box::pin(async move {
            let page = self.instances.list(&query).await?;
            Ok(InstancePage {
                instances: page
                    .instances
                    .into_iter()
                    .map(instance_info)
                    .collect::<Result<_, _>>()?,
                next_after: page.next_after,
            })
        })
    }
    fn remove(&self, id: InstanceId) -> DriverFuture<'_, RemoveInstanceOutcome> {
        Box::pin(async move {
            Ok(if self.instances.remove(&id).await? {
                RemoveInstanceOutcome::Removed
            } else {
                RemoveInstanceOutcome::AlreadyAbsent
            })
        })
    }
}

impl WindowsLocalDriver {
    /// Forward to this driver's self-managed instance registry without opening storage.
    pub fn create_instance(&self, config: InstanceConfig) -> DriverFuture<'_, InstanceInfo> {
        ah_vfs::application::port::InstanceRegistry::create(self.registry.as_ref(), config)
    }
    /// Forward to this driver's self-managed instance registry without opening storage.
    pub fn get_instance(&self, id: InstanceId) -> DriverFuture<'_, Option<InstanceInfo>> {
        ah_vfs::application::port::InstanceRegistry::get(self.registry.as_ref(), id)
    }
    /// Forward to this driver's self-managed instance registry without opening storage.
    pub fn list_instances(&self, query: InstanceQuery) -> DriverFuture<'_, InstancePage> {
        ah_vfs::application::port::InstanceRegistry::list(self.registry.as_ref(), query)
    }
    /// Forward to this driver's self-managed instance registry without opening storage.
    pub fn remove_instance(&self, id: InstanceId) -> DriverFuture<'_, RemoveInstanceOutcome> {
        ah_vfs::application::port::InstanceRegistry::remove(self.registry.as_ref(), id)
    }
}

impl InstanceConfigRow for LocalConfig {
    const TABLE: &'static str = "local_instances";
    const COLUMNS: &'static [&'static str] = &["root_path"];
    const SCHEMA: &'static str = include_str!("driver/sqlite/schema.sql");
    fn decode(row: &SqliteRow) -> Result<Self, DriverError> {
        let root: PathBuf = row
            .try_get::<String, _>("root_path")
            .map_err(vfs_driver_kit::database_error)?
            .into();
        if !root.is_absolute()
            || root.components().any(|part| {
                matches!(
                    part,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )
            })
        {
            return Err(WindowsLocalDriverError::InvalidData.into());
        }
        Ok(Self { root })
    }
    fn bind<'a>(&'a self, query: SqliteInsert<'a>) -> Result<SqliteInsert<'a>, DriverError> {
        Ok(query.bind(
            self.root
                .to_str()
                .ok_or(WindowsLocalDriverError::InvalidPathEncoding)?,
        ))
    }
}
