//! Stateless local-directory validation and storage opening.
use crate::{LinuxLocalDriverError, LinuxLocalStorage};
use ah_vfs::application::driver_error::{DriverError, InstanceConfigErrorKind};
use ah_vfs::application::driver_info::DriverInfo;
use ah_vfs::application::dto::{
    InstanceConfig, InstanceConfigSchema, instance_config_string, validate_instance_config_fields,
};
use ah_vfs::application::port::{
    Driver, DriverFuture, InstanceProvisioner, OpenContext, PreparedConfig, Storage,
};
use ah_vfs::domain::DriverKey;
use ah_vfs::domain::namespace::ChildMountPolicy;
use ah_vfs_storage::{
    AttributeSupport, Capabilities, DirectorySupport, ReadSupport, RenameSupport, WriteModes,
    WriteSupport,
};

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

/// Local filesystem driver; instance records belong to the host.
pub struct LinuxLocalDriver {
    info: DriverInfo,
}
impl LinuxLocalDriver {
    fn supported_capabilities() -> Capabilities {
        Capabilities::default()
            .with_stat(true)
            .with_read_link(true)
            .with_list(true)
            .with_read(ReadSupport::Range)
            .with_write(WriteSupport::staged(WriteModes::CreateNew, true))
            .with_directories(DirectorySupport::hierarchical(true, true))
            .with_remove(true)
            .with_rename(RenameSupport::Both)
            .with_permissions(AttributeSupport::ReadWrite)
            .with_xattrs(AttributeSupport::ReadWrite)
    }

    pub const KEY: &str = "local";
    pub fn new() -> Result<Self, LinuxLocalDriverError> {
        Ok(Self {
            info: DriverInfo::builder()
                .maybe_instance_config_schema(instance_config_schema())
                .capabilities(Self::supported_capabilities())
                .key(DriverKey::parse(Self::KEY).map_err(LinuxLocalDriverError::from)?)
                .name("Local filesystem".into())
                .version(semver::Version::new(0, 1, 0))
                .child_mount_policy(ChildMountPolicy::Allow)
                .build(),
        })
    }
}
async fn canonical_dir(source: &Path) -> Result<PathBuf, LinuxLocalDriverError> {
    let root = tokio::fs::canonicalize(source)
        .await
        .map_err(|source_error| LinuxLocalDriverError::Dir {
            path: source.to_path_buf(),
            source: source_error,
        })?;
    let metadata =
        tokio::fs::metadata(&root)
            .await
            .map_err(|source| LinuxLocalDriverError::Dir {
                path: root.clone(),
                source,
            })?;
    if !metadata.is_dir() {
        return Err(LinuxLocalDriverError::NotDir(root));
    }
    Ok(root)
}

impl Driver for LinuxLocalDriver {
    fn provisioner(&self) -> Arc<dyn InstanceProvisioner> {
        Arc::new(LocalProvisioner)
    }
    fn info(&self) -> &DriverInfo {
        &self.info
    }
    fn open(&self, context: OpenContext) -> DriverFuture<'_, Arc<dyn Storage>> {
        Box::pin(async move {
            let config = context.config();
            let source = instance_config_string(config, "root_path")?;
            let root = canonical_dir(Path::new(source))
                .await
                .map_err(DriverError::from)?;
            if root != Path::new(source) {
                return Err(LinuxLocalDriverError::InvalidData.into());
            }
            Ok(Arc::new(
                LinuxLocalStorage::open(root)
                    .await
                    .map_err(DriverError::from)?,
            ) as Arc<dyn Storage>)
        })
    }
}
struct LocalProvisioner;
impl InstanceProvisioner for LocalProvisioner {
    fn prepare(&self, config: InstanceConfig) -> DriverFuture<'_, PreparedConfig> {
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
            let root = root
                .to_str()
                .ok_or(LinuxLocalDriverError::InvalidPathEncoding)?;
            let config = InstanceConfig::from([("root_path".into(), root.into())]);
            Ok(PreparedConfig::new(config.clone(), config.into_object()))
        })
    }
}
fn instance_config_schema() -> Option<InstanceConfigSchema> {
    Some(BTreeMap::from([
        ("type".into(), "object".into()),
        (
            "properties".into(),
            serde_json::json!({"root_path": {"x-immutable": true, "type": "string", "minLength": 1, "description": "Existing UTF-8 directory; relative paths use the process working directory"}}),
        ),
        ("required".into(), serde_json::json!(["root_path"])),
        ("additionalProperties".into(), false.into()),
    ]))
}
