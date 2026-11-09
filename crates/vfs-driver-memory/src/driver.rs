use crate::{Capacity, CapacityUnit, MemoryDriverError, MemoryStorage};
use ah_vfs::application::driver_error::{DriverError, InstanceConfigErrorKind};
use ah_vfs::application::driver_error::{LifecycleError, LifecycleFailure};
use ah_vfs::application::driver_info::DriverInfo;
use ah_vfs::application::dto::{
    InstanceConfig, InstanceConfigSchema, instance_config_string, instance_config_u64,
    validate_instance_config_fields,
};
use ah_vfs::application::port::{
    Driver, DriverFuture, InstanceProvisioner, OpenContext, PreparedConfig, Storage,
};
use ah_vfs::domain::DriverKey;
use ah_vfs::domain::namespace::ChildMountPolicy;

use ah_vfs_storage::{
    Capabilities, DirectorySupport, ReadSupport, RenameSupport, WriteModes, WriteSupport,
};
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::Mutex;

/// Repeated opens on this driver share volatile contents. Deleting an instance
/// retains its data until the driver is dropped. Existing handles may outlive it.
pub struct MemoryDriver {
    info: DriverInfo,
    opened: Mutex<BTreeMap<String, Arc<MemoryStorage>>>,
}
impl MemoryDriver {
    fn supported_capabilities() -> Capabilities {
        Capabilities::default()
            .with_stat(true)
            .with_list(true)
            .with_read(ReadSupport::Range)
            .with_write(WriteSupport::staged(WriteModes::Both, true))
            .with_directories(DirectorySupport::hierarchical(true, true))
            .with_remove(true)
            .with_rename(RenameSupport::Both)
    }

    pub const KEY: &'static str = "memory";
    pub fn new() -> Result<Self, MemoryDriverError> {
        Ok(Self {
            info: DriverInfo::builder()
                .key(DriverKey::parse(Self::KEY).map_err(MemoryDriverError::from)?)
                .name("Memory storage".into())
                .version(semver::Version::new(0, 1, 0))
                .child_mount_policy(ChildMountPolicy::Allow)
                .maybe_instance_config_schema(instance_config_schema())
                .capabilities(Self::supported_capabilities())
                .build(),
            opened: Mutex::default(),
        })
    }
}
impl Driver for MemoryDriver {
    fn provisioner(&self) -> Arc<dyn InstanceProvisioner> {
        Arc::new(MemoryProvisioner)
    }
    fn info(&self) -> &DriverInfo {
        &self.info
    }
    fn open(&self, context: OpenContext) -> DriverFuture<'_, Arc<dyn Storage>> {
        Box::pin(async move {
            let config = context.config();
            let capacity = capacity(config)?;
            let mut opened = self.opened.lock().await;
            let storage = match opened.entry(context.instance_id().as_str().into()) {
                std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(Arc::new(MemoryStorage::new(capacity)))
                }
            };
            if storage.capacity() != capacity {
                return Err(LifecycleError::new(LifecycleFailure::InvalidRecord).into());
            }
            Ok(storage.clone() as Arc<dyn Storage>)
        })
    }
}
fn capacity(config: &InstanceConfig) -> Result<Capacity, DriverError> {
    validate_instance_config_fields(config, &["capacity_size", "capacity_unit"])?;
    let size = instance_config_u64(config, "capacity_size")?;
    let unit: CapacityUnit = instance_config_string(config, "capacity_unit")?
        .parse()
        .map_err(|_| DriverError::InvalidInstanceConfig {
            field: "capacity_unit".into(),
            kind: InstanceConfigErrorKind::InvalidValue,
        })?;
    Capacity::new(size, unit).map_err(|_| DriverError::InvalidInstanceConfig {
        field: "capacity_size".into(),
        kind: InstanceConfigErrorKind::InvalidValue,
    })
}
struct MemoryProvisioner;
impl InstanceProvisioner for MemoryProvisioner {
    fn prepare(&self, config: InstanceConfig) -> DriverFuture<'_, PreparedConfig> {
        Box::pin(async move {
            let capacity = capacity(&config)?;
            let details = BTreeMap::from([
                ("capacity_size".into(), capacity.size().into()),
                ("capacity_unit".into(), capacity.unit().as_str().into()),
            ]);
            Ok(PreparedConfig::new(config, details))
        })
    }
}
fn instance_config_schema() -> Option<InstanceConfigSchema> {
    Some(BTreeMap::from([
        ("type".into(), "object".into()),
        (
            "properties".into(),
            serde_json::json!({
                "capacity_size": {"x-immutable": true, "type": "integer", "minimum": 1, "maximum": i64::MAX, "format": "uint64"},
                "capacity_unit": {"x-immutable": true, "type": "string", "enum": ["B", "KiB", "MiB", "GiB", "TiB"]}
            }),
        ),
        (
            "required".into(),
            serde_json::json!(["capacity_size", "capacity_unit"]),
        ),
        ("additionalProperties".into(), false.into()),
    ]))
}
