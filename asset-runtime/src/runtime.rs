use asset_core::CoreError;
use asset_core::port::{DirectoryKindRegistry, ResourceKindRegistry, SecurityAuditRepository};
use asset_core::service::{AuthorizationService, ResourceService, UserService};
use asset_infra::AssetInfrastructure;
use asset_infra::config::AssetInfraConfig;
use asset_infra::storage::LocalStorageSync;
use asset_plugin_api::PluginWebAssets;
use sqlx::SqlitePool;
use std::sync::Arc;

/// 应用运行时。
///
/// `AssetRuntime` 负责根据调用方已经加载的配置组装基础设施与核心 service，并持有由
/// 应用入口显式启动的后台任务。配置来源、命令行参数和传输层生命周期由各应用自行决定。
pub struct AssetRuntime {
    /// 已初始化的基础设施组合。
    infrastructure: AssetInfrastructure,
    /// 保持自动存储同步监听器与后台任务存活。
    storage_sync: Option<LocalStorageSync>,
}

impl AssetRuntime {
    /// 使用调用方提供的配置组装应用运行时。
    ///
    /// 创建运行时时不会自动启动后台任务；长生命周期应用应按需显式调用
    /// [`AssetRuntime::start_storage_sync`]。
    pub async fn new(config: AssetInfraConfig) -> Result<Self, CoreError> {
        let infrastructure = AssetInfrastructure::new(config).await?;
        Ok(Self {
            infrastructure,
            storage_sync: None,
        })
    }

    /// 启动配置所指定的自动存储同步任务，并由运行时持有其生命周期。
    ///
    /// 重复调用不会创建第二个同步任务。配置禁用同步时该方法成功返回但不启动任务。
    pub async fn start_storage_sync(&mut self) -> Result<(), CoreError> {
        if self.storage_sync.is_none() {
            self.storage_sync = self
                .infrastructure
                .start_storage_sync(self.infrastructure.resource_service())
                .await?;
        }
        Ok(())
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

    /// 返回由基础设施统一创建的数据库连接池。
    pub fn database_pool(&self) -> SqlitePool {
        self.infrastructure.database_pool()
    }

    /// 返回资源类型注册表。
    pub fn resource_kind_registry(&self) -> Arc<dyn ResourceKindRegistry> {
        self.infrastructure.resource_kind_registry()
    }

    /// 返回目录类型注册表。
    pub fn directory_kind_registry(&self) -> Arc<dyn DirectoryKindRegistry> {
        self.infrastructure.directory_kind_registry()
    }

    /// 返回启动时校验并冻结的插件浏览器静态资源。
    pub fn plugin_web_assets(&self) -> PluginWebAssets {
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
                return Some(resource.into_resource());
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
        let mut runtime = AssetRuntime::new(config).await.unwrap();
        runtime.start_storage_sync().await.unwrap();
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
            if resource.resource().content().unwrap().size() == 14 {
                updated = Some(resource);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let updated = updated.expect("modified file should update Resource content");
        assert_eq!(updated.resource().id(), first.id());

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
                .directory_index()
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
                .directory_index()
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
