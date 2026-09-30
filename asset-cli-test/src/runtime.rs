//! Application configuration and service assembly, intended for extraction
//! into a future runtime crate. This module is independent of CLI parsing.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use asset_infra::driver::{LocalDriver, MemoryDriver};
use asset_infra::repository::SqliteMountRepository;
use asset_vfs::MountService;
use asset_vfs::config::{DatabaseKind, VfsConfig};
use asset_vfs::driver::{Driver, DriverKind, DriverPath};
use asset_vfs::error::VfsError;
use asset_vfs::mount::{Mount, MountId, MountRepository};
use asset_vfs::namespace::VirtualPath;

pub type RuntimeError = Box<dyn Error + Send + Sync>;

/// Startup options, separate from clap and persisted configuration sections.
pub struct RuntimeOptions {
    pub config_file: PathBuf,
}

impl Default for RuntimeOptions {
    fn default() -> Self {
        Self {
            config_file: PathBuf::from("config.toml"),
        }
    }
}

pub struct Runtime {
    mount_service: MountService,
    driver_metadata: [DriverMetadata; 2],
}

/// CLI view of a persisted mount and its driver's declared submount policy.
pub struct MountView {
    pub mount: Mount,
    /// `None` when the driver kind is not known to this runtime.
    pub allows_submounts: Option<bool>,
}

// Display metadata only; obtaining it does not bind or retain a backend.
struct DriverMetadata {
    kind: DriverKind,
    allows_submounts: bool,
}

impl DriverMetadata {
    fn from_driver(driver: &dyn Driver) -> Self {
        Self {
            kind: driver.kind(),
            allows_submounts: driver.allows_submounts(),
        }
    }
}

impl Runtime {
    /// Loads configuration and opens storage. Relative paths use the process
    /// working directory, not the configuration file's parent directory.
    /// Ensures an enabled, persisted local root mount matches configuration.
    pub async fn initialize(options: RuntimeOptions) -> Result<Self, RuntimeError> {
        let loaded = asset_config::load(&options.config_file)?;
        let config = loaded.get::<VfsConfig>()?;
        if config.config_dir.as_os_str().is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "vfs.config_dir must not be empty",
            )
            .into());
        }
        if config.root_mount_path.as_os_str().is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "vfs.root_mount_path must not be empty",
            )
            .into());
        }
        let root = DriverPath::new(config.root_mount_path.to_str().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "vfs.root_mount_path must be valid UTF-8",
            )
        })?);
        std::fs::create_dir_all(&config.root_mount_path)?;
        LocalDriver.bind(&root)?;
        std::fs::create_dir_all(&config.config_dir)?;
        let repository: Arc<dyn MountRepository> = match config.database {
            DatabaseKind::Sqlite => {
                Arc::new(SqliteMountRepository::open(config.sqlite_path()).await?)
            }
        };
        ensure_root_mount(repository.as_ref(), root).await?;
        Ok(Self {
            mount_service: MountService::new(repository),
            driver_metadata: [
                DriverMetadata::from_driver(&LocalDriver),
                DriverMetadata::from_driver(&MemoryDriver::new()),
            ],
        })
    }

    pub fn mount_service(&self) -> &MountService {
        &self.mount_service
    }

    /// Lists persisted mounts with display metadata, without binding their drivers.
    pub async fn list_mounts(&self) -> Result<Vec<MountView>, VfsError> {
        Ok(self
            .mount_service()
            .list_mounts()
            .await?
            .into_iter()
            .map(|mount| self.mount_view(mount))
            .collect())
    }

    /// Loads one persisted mount with the same display metadata used by listing.
    pub async fn mount_info(&self, id: MountId) -> Result<Option<MountView>, VfsError> {
        Ok(self
            .mount_service()
            .mount_info(id)
            .await?
            .map(|mount| self.mount_view(mount)))
    }

    fn mount_view(&self, mount: Mount) -> MountView {
        let allows_submounts = self
            .driver_metadata
            .iter()
            .find(|metadata| &metadata.kind == mount.driver())
            .map(|metadata| metadata.allows_submounts);
        MountView {
            mount,
            allows_submounts,
        }
    }
}

/// Runtime policy: the configured local root must exist and be enabled.
async fn ensure_root_mount(
    repository: &dyn MountRepository,
    root: DriverPath,
) -> Result<(), RuntimeError> {
    let mount = Mount::new(
        MountId::new(),
        VirtualPath::root(),
        LocalDriver.kind(),
        root,
        true,
    );
    repository.upsert_by_path(&mount).await?;
    Ok(())
}

impl RuntimeOptions {
    pub fn with_config_file(config_file: impl AsRef<Path>) -> Self {
        Self {
            config_file: config_file.as_ref().to_owned(),
        }
    }
}
