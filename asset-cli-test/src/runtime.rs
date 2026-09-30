//! Application configuration and service assembly, intended for extraction
//! into a future runtime crate. This module is independent of CLI parsing.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use asset_infra::driver::LocalDriver;
use asset_infra::repository::SqliteMountRepository;
use asset_vfs::MountService;
use asset_vfs::config::{DatabaseKind, VfsConfig};
use asset_vfs::driver::{Driver, DriverKind, DriverPath};
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
        })
    }

    pub fn mount_service(&self) -> &MountService {
        &self.mount_service
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
        DriverKind::try_from("local")?,
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
