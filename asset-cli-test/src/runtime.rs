//! Application configuration and service assembly, intended for extraction
//! into a future runtime crate. This module is independent of CLI parsing.

mod config;

use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use asset_infra::driver::{LocalDriver, MemoryDriver};
use asset_infra::repository::SqliteMountRepository;
use asset_vfs::driver::{Driver, DriverKind, DriverPath};
use asset_vfs::mount::{Mount, MountId, MountRepository};
use asset_vfs::namespace::VirtualPath;
use asset_vfs::{DriverService, MountService};

use config::{AssetConfig, DatabaseKind};

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
    driver_service: Arc<DriverService>,
    mount_service: MountService,
}

impl Runtime {
    /// Loads configuration and opens storage. Relative paths use the process
    /// working directory, not the configuration file's parent directory.
    /// Ensures an enabled, persisted local root mount matches configuration.
    pub async fn initialize(options: RuntimeOptions) -> Result<Self, RuntimeError> {
        let loaded = asset_config::load(&options.config_file)?;
        let config = loaded.get::<AssetConfig>()?;
        if config.config_dir.as_os_str().is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "asset.config_dir must not be empty",
            )
            .into());
        }
        if config.root_mount_path.as_os_str().is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "asset.root_mount_path must not be empty",
            )
            .into());
        }
        let root = DriverPath::new(config.root_mount_path.to_str().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "asset.root_mount_path must be valid UTF-8",
            )
        })?);
        let local = Arc::new(LocalDriver);
        let root_driver_kind = local.kind();
        let factories: Vec<Arc<dyn Driver>> = vec![local.clone(), Arc::new(MemoryDriver::new())];
        let drivers = Arc::new(DriverService::new(factories)?);

        std::fs::create_dir_all(&config.root_mount_path)?;
        // The runtime selects the local root and prepares its native directory.
        // Ordinary driver queries remain independent of backend availability.
        local.bind(&root)?;
        std::fs::create_dir_all(&config.config_dir)?;
        let repository: Arc<dyn MountRepository> = match config.database {
            DatabaseKind::Sqlite => {
                Arc::new(SqliteMountRepository::open(config.sqlite_path()).await?)
            }
        };
        ensure_root_mount(repository.as_ref(), root_driver_kind, root).await?;
        Ok(Self {
            driver_service: Arc::clone(&drivers),
            mount_service: MountService::new(repository, drivers),
        })
    }

    pub fn driver_service(&self) -> &DriverService {
        &self.driver_service
    }

    pub fn mount_service(&self) -> &MountService {
        &self.mount_service
    }
}

/// Runtime policy: the configured local root must exist and be enabled.
async fn ensure_root_mount(
    repository: &dyn MountRepository,
    driver: DriverKind,
    root: DriverPath,
) -> Result<(), RuntimeError> {
    let mount = Mount::new(MountId::new(), VirtualPath::root(), driver, root, true);
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
