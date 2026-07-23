use asset_core::CoreError;
use asset_core::port::{ResourceKindRegistry, SecurityAuditRepository};
use asset_core::service::{AuthorizationService, ResourceService, UserService};
use asset_infra::AssetInfrastructure;
use asset_infra::config::{AssetInfraConfig, DEFAULT_CONFIG_FILE};
use asset_infra::storage::LocalStorageSync;
use std::path::Path;
use std::sync::Arc;

/// 应用运行时。
///
/// `AssetRuntime` 负责把配置、基础设施实现和核心 service 组装起来。
/// HTTP、CLI、TUI 等外部入口都应复用它，避免重复初始化 SQLite、Fs 等依赖。
pub struct AssetRuntime {
    /// 已初始化的基础设施组合。
    infrastructure: AssetInfrastructure,
    /// 保持自动存储同步监听器与后台任务存活。
    _storage_sync: Option<LocalStorageSync>,
}

impl AssetRuntime {
    /// 使用默认配置文件创建应用运行时。
    ///
    /// 当前默认配置文件名是 `config.toml`。文件不存在时使用默认配置。
    pub async fn from_default_config_file() -> Result<Self, CoreError> {
        Self::from_config(AssetInfraConfig::from_default_config_file()?).await
    }

    /// 创建不启动自动存储同步的短生命周期维护运行时。
    pub async fn from_default_config_file_without_storage_sync() -> Result<Self, CoreError> {
        Self::from_config_inner(AssetInfraConfig::from_default_config_file()?, false).await
    }

    /// 使用显式配置创建应用运行时。
    pub async fn from_config(config: AssetInfraConfig) -> Result<Self, CoreError> {
        Self::from_config_inner(config, true).await
    }

    async fn from_config_inner(
        config: AssetInfraConfig,
        start_storage_sync: bool,
    ) -> Result<Self, CoreError> {
        let infrastructure = AssetInfrastructure::new(config).await?;
        let storage_sync = if start_storage_sync {
            infrastructure
                .start_storage_sync(infrastructure.resource_service())
                .await?
        } else {
            None
        };

        Ok(Self {
            infrastructure,
            _storage_sync: storage_sync,
        })
    }

    /// 使用可选配置文件创建应用运行时。
    ///
    /// `path` 为 `Some` 时读取指定配置文件，文件不存在会返回错误。
    /// `path` 为 `None` 时读取默认 `config.toml`，文件不存在则使用默认配置。
    pub async fn from_optional_config_file(
        path: Option<impl AsRef<Path>>,
    ) -> Result<Self, CoreError> {
        match path {
            Some(path) => Self::from_config(AssetInfraConfig::from_config_file(path)?).await,
            None => Self::from_default_config_file().await,
        }
    }

    /// 返回默认配置文件名。
    pub fn default_config_file() -> &'static str {
        DEFAULT_CONFIG_FILE
    }

    /// 返回实际生效的基础设施配置。
    pub fn config(&self) -> &AssetInfraConfig {
        self.infrastructure.config()
    }

    /// 创建资源应用服务。
    pub fn resource_service(&self) -> ResourceService {
        self.infrastructure.resource_service()
    }

    pub fn user_service(&self) -> UserService {
        self.infrastructure.user_service()
    }

    pub fn authorization_service(&self) -> AuthorizationService {
        self.infrastructure.authorization_service()
    }

    pub fn security_audit_repository(&self) -> Arc<dyn SecurityAuditRepository> {
        self.infrastructure.security_audit_repository()
    }

    /// 返回资源类型注册表。
    pub fn resource_kind_registry(&self) -> Arc<dyn ResourceKindRegistry> {
        self.infrastructure.resource_kind_registry()
    }

    /// 返回启动时校验并冻结的插件浏览器静态资源。
    pub fn plugin_web_assets(&self) -> asset_infra::PluginWebAssets {
        self.infrastructure.plugin_web_assets()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use asset_core::domain::{DirectoryPath, Resource};
    use asset_infra::config::{
        BlobConfig, DatabaseConfig, LocalBlobConfig, LocalBlobSyncConfig, SqliteDatabaseConfig,
    };
    use std::time::Duration;

    async fn wait_for_resource(
        runtime: &AssetRuntime,
        directory: &DirectoryPath,
        name: &str,
    ) -> Option<Resource> {
        for _ in 0..100 {
            if let Some(resource) = runtime
                .infrastructure
                .resource_query()
                .find_by_path(directory, name)
                .await
                .unwrap()
            {
                return Some(resource);
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        None
    }

    async fn wait_until_absent(runtime: &AssetRuntime, directory: &DirectoryPath, name: &str) {
        for _ in 0..100 {
            if runtime
                .infrastructure
                .resource_query()
                .find_by_path(directory, name)
                .await
                .unwrap()
                .is_none()
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("resource `{directory}/{name}` was not removed by automatic synchronization");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn local_storage_changes_are_synchronized_automatically() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "asset-hub-auto-sync-{}-{nonce}",
            std::process::id()
        ));
        let config = AssetInfraConfig {
            database: DatabaseConfig {
                sqlite: SqliteDatabaseConfig { max_connections: 1 },
                ..DatabaseConfig::default()
            },
            blob: BlobConfig {
                local: LocalBlobConfig {
                    root: root.clone(),
                    sync: LocalBlobSyncConfig {
                        enabled: true,
                        debounce_milliseconds: 50,
                        reconcile_interval_seconds: 1,
                    },
                },
                ..BlobConfig::default()
            },
            ..AssetInfraConfig::default()
        };
        let runtime = AssetRuntime::from_config(config).await.unwrap();
        let directory = DirectoryPath::from_path("documents").unwrap();
        let directory_path = root.join("documents");
        std::fs::create_dir_all(&directory_path).unwrap();
        std::fs::write(directory_path.join("note.txt"), b"first").unwrap();

        let first = wait_for_resource(&runtime, &directory, "note.txt")
            .await
            .expect("new file should be imported automatically");
        std::fs::write(directory_path.join("note.txt"), b"second version").unwrap();
        let mut updated = None;
        for _ in 0..100 {
            let resource = runtime
                .infrastructure
                .resource_query()
                .find_by_path(&directory, "note.txt")
                .await
                .unwrap()
                .unwrap();
            if resource.content().unwrap().size() == 14 {
                updated = Some(resource);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let updated = updated.expect("modified file should update Resource content");
        assert_eq!(updated.id(), first.id());

        std::fs::rename(
            directory_path.join("note.txt"),
            directory_path.join("renamed.txt"),
        )
        .unwrap();
        let renamed = wait_for_resource(&runtime, &directory, "renamed.txt")
            .await
            .expect("renamed file should be synchronized automatically");
        assert_eq!(renamed.id(), first.id());

        std::fs::remove_file(directory_path.join("renamed.txt")).unwrap();
        wait_until_absent(&runtime, &directory, "renamed.txt").await;

        let empty_directory = root.join("empty");
        std::fs::create_dir(&empty_directory).unwrap();
        let mut directory_created = false;
        for _ in 0..100 {
            if runtime
                .infrastructure
                .directory_repository()
                .list_children(&asset_core::domain::DirectoryId::root())
                .await
                .unwrap()
                .iter()
                .any(|directory| directory.path().path() == "empty")
            {
                directory_created = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(directory_created, "new directory should be synchronized");
        std::fs::remove_dir(&empty_directory).unwrap();
        let mut directory_removed = false;
        for _ in 0..100 {
            if runtime
                .infrastructure
                .directory_repository()
                .list_children(&asset_core::domain::DirectoryId::root())
                .await
                .unwrap()
                .iter()
                .all(|directory| directory.path().path() != "empty")
            {
                directory_removed = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(
            directory_removed,
            "removed directory should be synchronized"
        );

        drop(runtime);
        tokio::task::yield_now().await;
        let _ = std::fs::remove_dir_all(&root);
    }
}
