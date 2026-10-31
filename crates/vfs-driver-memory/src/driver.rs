use crate::{Capacity, CapacityUnit, MemoryDriverError, MemoryStorage};
use ah_vfs::{
    application::{
        driver_info::DriverInfo,
        dto::{
            InstanceConfig, InstanceConfigSchema, InstanceInfo, InstancePage, InstanceQuery,
            RemoveInstanceOutcome, instance_config_string, instance_config_u64,
            validate_instance_config_fields,
        },
        port::{Driver, DriverFuture, Storage},
    },
    domain::{ChildMountPolicy, DriverError, DriverKey, InstanceConfigErrorKind, InstanceId},
};
use sqlx::{Row, SqlitePool, sqlite::SqliteRow};
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::Mutex;
use vfs_driver_kit::{InstanceConfigRow, InstanceStore, SqliteInsert, StoredInstance};

/// Persisted instance identity and quota; file contents are never stored in SQLite.
type MemoryInstance = StoredInstance<Capacity>;

#[derive(Default)]
struct OpenedInstances {
    instances: BTreeMap<String, Arc<MemoryStorage>>,
    generation: Arc<()>,
}

/// Each driver object owns its runtime instances. Reopening on the same object
/// shares contents; constructing another driver starts with empty contents.
/// Existing storage/stream handles survive instance removal or driver destruction.
/// Instance queries expose numeric `details.capacity_size` and string
/// `details.capacity_unit`, retaining the configured binary unit.
pub struct MemoryDriver {
    info: DriverInfo,
    registry: Arc<MemoryRegistry>,
}

impl MemoryDriver {
    pub const KEY: &'static str = "memory";

    /// Initialize only memory-owned tables, retaining all existing settings.
    pub async fn from_pool(pool: SqlitePool) -> Result<Self, MemoryDriverError> {
        let store = InstanceStore::from_pool(pool)
            .await
            .map_err(MemoryDriverError::Registry)?;
        Ok(Self {
            info: DriverInfo::builder()
                .key(DriverKey::parse(Self::KEY).map_err(|_| MemoryDriverError::InvalidData)?)
                .name("Memory storage".into())
                .version(semver::Version::new(0, 1, 0))
                .child_mount_policy(ChildMountPolicy::Allow)
                .maybe_instance_config_schema(instance_config_schema())
                .capabilities(MemoryStorage::supported_capabilities())
                .build(),
            registry: Arc::new(MemoryRegistry {
                store,
                opened: Mutex::new(OpenedInstances::default()),
            }),
        })
    }
}

impl Driver for MemoryDriver {
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
        let id = context.instance_id().clone();
        let registry = self.registry.clone();
        Box::pin(async move {
            loop {
                let generation = registry.opened.lock().await.generation.clone();
                let instance = registry
                    .load_instance(&id)
                    .await?
                    .ok_or_else(|| DriverError::InstanceNotFound(id.clone()))?;
                let mut opened = registry.opened.lock().await;
                if !Arc::ptr_eq(&generation, &opened.generation) {
                    continue;
                }
                let storage = opened
                    .instances
                    .entry(id.as_str().into())
                    .or_insert_with(|| Arc::new(MemoryStorage::new(*instance.config())));
                if storage.capacity() != *instance.config() {
                    return Err(ah_vfs::domain::StorageError::new(
                        ah_vfs::domain::StorageErrorKind::InvalidData,
                    )
                    .into());
                }
                return Ok(storage.clone() as Arc<dyn Storage>);
            }
        })
    }
}

fn instance_info(instance: MemoryInstance) -> InstanceInfo {
    InstanceInfo {
        id: instance.id().clone(),
        details: BTreeMap::from([
            ("capacity_size".into(), instance.config().size().into()),
            (
                "capacity_unit".into(),
                instance.config().unit().as_str().into(),
            ),
        ]),
    }
}

impl MemoryRegistry {
    /// Read validated settings without opening or allocating runtime storage.
    async fn load_instance(&self, id: &InstanceId) -> Result<Option<MemoryInstance>, DriverError> {
        self.store.get(id).await
    }
}

struct MemoryRegistry {
    store: InstanceStore<Capacity>,
    opened: Mutex<OpenedInstances>,
}

fn instance_config_schema() -> Option<InstanceConfigSchema> {
    Some(BTreeMap::from([
        ("type".into(), "object".into()),
        (
            "properties".into(),
            serde_json::json!({
                "capacity_size": {"type": "integer", "minimum": 1, "maximum": i64::MAX, "format": "uint64"},
                "capacity_unit": {"type": "string", "enum": ["B", "KiB", "MiB", "GiB", "TiB"]}
            }),
        ),
        (
            "required".into(),
            serde_json::json!(["capacity_size", "capacity_unit"]),
        ),
        ("additionalProperties".into(), false.into()),
    ]))
}

impl ah_vfs::application::port::InstanceRegistry for MemoryRegistry {
    fn create(&self, config: InstanceConfig) -> DriverFuture<'_, InstanceInfo> {
        Box::pin(async move {
            validate_instance_config_fields(&config, &["capacity_size", "capacity_unit"])?;
            let size = instance_config_u64(&config, "capacity_size")?;
            let unit: CapacityUnit = instance_config_string(&config, "capacity_unit")?
                .parse()
                .map_err(|_| DriverError::InvalidInstanceConfig {
                    field: "capacity_unit".into(),
                    kind: InstanceConfigErrorKind::InvalidValue,
                })?;
            let capacity =
                Capacity::new(size, unit).map_err(|_| DriverError::InvalidInstanceConfig {
                    field: "capacity_size".into(),
                    kind: InstanceConfigErrorKind::InvalidValue,
                })?;
            Ok(instance_info(self.store.insert(capacity).await?))
        })
    }
    fn get(&self, id: InstanceId) -> DriverFuture<'_, Option<InstanceInfo>> {
        Box::pin(async move { Ok(self.load_instance(&id).await?.map(instance_info)) })
    }
    fn list(&self, query: InstanceQuery) -> DriverFuture<'_, InstancePage> {
        Box::pin(async move {
            let page = self.store.list(&query).await?;
            Ok(InstancePage {
                instances: page.instances.into_iter().map(instance_info).collect(),
                next_after: page.next_after,
            })
        })
    }
    fn remove(&self, id: InstanceId) -> DriverFuture<'_, RemoveInstanceOutcome> {
        Box::pin(async move {
            let removed = self.store.remove(&id).await?;
            let mut opened = self.opened.lock().await;
            // A pending open must recheck persistence after any removal.
            opened.generation = Arc::new(());
            // Also evict stale retention when settings were removed externally.
            opened.instances.remove(id.as_str());
            Ok(if !removed {
                RemoveInstanceOutcome::AlreadyAbsent
            } else {
                RemoveInstanceOutcome::Removed
            })
        })
    }
}

impl MemoryDriver {
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

impl InstanceConfigRow for Capacity {
    const TABLE: &'static str = "memory_instances";
    const COLUMNS: &'static [&'static str] = &["capacity_size", "capacity_unit"];
    const SCHEMA: &'static str = include_str!("driver/schema.sql");
    fn decode(row: &SqliteRow) -> Result<Self, DriverError> {
        let size: i64 = row
            .try_get("capacity_size")
            .map_err(vfs_driver_kit::database_error)?;
        let unit: String = row
            .try_get("capacity_unit")
            .map_err(vfs_driver_kit::database_error)?;
        Self::new(
            u64::try_from(size).map_err(|_| MemoryDriverError::InvalidData)?,
            unit.parse().map_err(|_| MemoryDriverError::InvalidData)?,
        )
        .map_err(|_| MemoryDriverError::InvalidData.into())
    }
    fn bind<'a>(&'a self, query: SqliteInsert<'a>) -> Result<SqliteInsert<'a>, DriverError> {
        Ok(query
            .bind(i64::try_from(self.size()).map_err(|_| MemoryDriverError::InvalidData)?)
            .bind(self.unit().as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{future::poll_fn, task::Poll};

    #[tokio::test]
    async fn persistence_waits_do_not_hold_the_runtime_map_lock()
    -> Result<(), Box<dyn std::error::Error>> {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await?;
        let driver = MemoryDriver::from_pool(pool.clone()).await?;
        let instance = driver
            .create_instance(InstanceConfig::from([
                ("capacity_size".into(), 1.into()),
                ("capacity_unit".into(), "KiB".into()),
            ]))
            .await?;
        let connection = pool.acquire().await?;
        let mut open = driver.open(instance.id.clone().into());
        assert!(
            poll_fn(|cx| Poll::Ready(open.as_mut().poll(cx)))
                .await
                .is_pending()
        );
        assert!(driver.registry.opened.try_lock().is_ok());
        drop(open);
        let mut remove = driver.remove_instance(instance.id);
        assert!(
            poll_fn(|cx| Poll::Ready(remove.as_mut().poll(cx)))
                .await
                .is_pending()
        );
        assert!(driver.registry.opened.try_lock().is_ok());
        drop(remove);
        drop(connection);
        Ok(())
    }

    #[tokio::test]
    async fn concurrent_opens_retain_the_same_storage() -> Result<(), Box<dyn std::error::Error>> {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await?;
        let driver = MemoryDriver::from_pool(pool).await?;
        let instance = driver
            .create_instance(InstanceConfig::from([
                ("capacity_size".into(), 1.into()),
                ("capacity_unit".into(), "KiB".into()),
            ]))
            .await?;
        let (one, two) = tokio::join!(
            driver.open(instance.id.clone().into()),
            driver.open(instance.id.into())
        );
        assert!(Arc::ptr_eq(&one?, &two?));
        Ok(())
    }
}
