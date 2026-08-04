use crate::PluginWebAssets;
use asset_core::CoreError;
use asset_core::domain::ResourceActionPolicy;
use asset_core::port::{DirectoryKindRegistry, ResourceKindRegistry, SecurityAuditRepository};
use asset_core::service::{
    AuthorizationService, DirectoryService, ResourceService, ResourceServicePorts, UserService,
};
use asset_infra::AssetInfrastructure;
use asset_infra::action::{DefaultDirectoryActionExecutor, DefaultResourceActionExecutor};
use asset_infra::config::AssetInfraConfig;
use asset_infra::kind::{
    DefaultDirectoryActionRegistry, DefaultDirectoryKindRegistry, DefaultResourceActionRegistry,
    DefaultResourceKindRegistry, directory_action_registry_from_catalog, registries_from_catalog,
};
use asset_infra::password::Argon2PasswordHasher;
use asset_infra::plugin::{ExtismActionExecutor, ExtismHost};
use asset_infra::plugin_package::PluginCatalog;
use asset_infra::storage::LocalStorageSync;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

/// 应用运行时。
///
/// `AssetRuntime` 负责根据调用方已经加载的配置组装基础设施与核心 service，并持有由
/// 应用入口显式启动的后台任务。配置来源、命令行参数和传输层生命周期由各应用自行决定。
pub struct AssetRuntime {
    /// 已初始化的基础设施组合。
    infrastructure: AssetInfrastructure,
    _plugin_catalog: PluginCatalog,
    resource_kind_registry: Arc<DefaultResourceKindRegistry>,
    directory_kind_registry: Arc<DefaultDirectoryKindRegistry>,
    _resource_action_registry: Arc<DefaultResourceActionRegistry>,
    _directory_action_registry: Arc<DefaultDirectoryActionRegistry>,
    _resource_action_executor: Arc<DefaultResourceActionExecutor>,
    _directory_action_executor: Arc<DefaultDirectoryActionExecutor>,
    plugin_web_assets: PluginWebAssets,
    resource_service: ResourceService,
    user_service: UserService,
    authorization_service: AuthorizationService,
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
        let catalog_started = Instant::now();
        let plugin_catalog = PluginCatalog::load(&infrastructure.config().plugin_packages_path())?;
        tracing::info!(
            elapsed_ms = catalog_started.elapsed().as_millis(),
            plugins = plugin_catalog.plugin_count(),
            "plugin artifacts verified"
        );

        let (resource_kind_registry, directory_kind_registry, resource_action_registry) =
            registries_from_catalog(&plugin_catalog)?;
        let resource_kind_registry = Arc::new(resource_kind_registry);
        let directory_kind_registry = Arc::new(directory_kind_registry);
        let resource_action_registry = Arc::new(resource_action_registry);
        let directory_action_registry =
            Arc::new(directory_action_registry_from_catalog(&plugin_catalog)?);
        let plugin_execution_policy = Arc::new(infrastructure.config().plugin.execution_policy()?);
        let resource_action_policy = Arc::new(
            ResourceActionPolicy::new(
                plugin_execution_policy.max_content_bytes(),
                plugin_execution_policy.max_inline_content_bytes(),
            )
            .map_err(|error| CoreError::configuration(error.to_string()))?,
        );

        let compile_started = Instant::now();
        let extism_action_executor = ExtismActionExecutor::from_catalog(
            &plugin_catalog,
            resource_kind_registry.as_ref(),
            directory_kind_registry.as_ref(),
            ExtismHost::new(
                infrastructure.directory_query(),
                infrastructure.resource_query(),
                infrastructure.blob_storage(),
                plugin_execution_policy.clone(),
                infrastructure.config().plugin.grants.clone(),
            ),
        )?;
        let directory_action_executor = Arc::new(DefaultDirectoryActionExecutor::new(
            &plugin_catalog,
            directory_kind_registry.as_ref(),
            extism_action_executor.clone(),
        ));
        let resource_action_executor = Arc::new(DefaultResourceActionExecutor::new(
            &plugin_catalog,
            resource_kind_registry.as_ref(),
            extism_action_executor,
        ));
        tracing::info!(
            elapsed_ms = compile_started.elapsed().as_millis(),
            "plugins compiled"
        );

        let directory_service = DirectoryService::new(
            infrastructure.directory_store(),
            infrastructure.directory_index(),
            infrastructure.directory_storage(),
            directory_kind_registry.clone(),
        )
        .with_actions(
            directory_action_registry.clone(),
            directory_action_executor.clone(),
        );
        let resource_service = ResourceService::new(
            ResourceServicePorts::new(
                infrastructure.resource_repository(),
                infrastructure.resource_query(),
                infrastructure.blob_storage(),
                infrastructure.directory_store(),
                infrastructure.directory_index(),
                infrastructure.directory_storage(),
                directory_kind_registry.clone(),
                infrastructure.storage_scanner(),
                resource_kind_registry.clone(),
                infrastructure.upload_session_repository(),
            )
            .with_actions(
                resource_action_registry.clone(),
                resource_action_executor.clone(),
            )
            .with_directory_actions(
                directory_action_registry.clone(),
                directory_action_executor.clone(),
            ),
            resource_action_policy,
        );
        let user_service = UserService::new(
            infrastructure.user_repository(),
            infrastructure.user_query(),
            Arc::new(Argon2PasswordHasher),
            directory_service.clone(),
        );
        let authorization_service =
            AuthorizationService::new(infrastructure.user_repository(), directory_service);
        let plugin_web_assets = plugin_web_assets_from_catalog(&plugin_catalog)?;

        let resumed = resource_service.resume_upload_finalizations().await?;
        if resumed > 0 {
            tracing::info!(count = resumed, "resumed pending upload finalizations");
        }
        Ok(Self {
            infrastructure,
            _plugin_catalog: plugin_catalog,
            resource_kind_registry,
            directory_kind_registry,
            _resource_action_registry: resource_action_registry,
            _directory_action_registry: directory_action_registry,
            _resource_action_executor: resource_action_executor,
            _directory_action_executor: directory_action_executor,
            plugin_web_assets,
            resource_service,
            user_service,
            authorization_service,
            storage_sync: None,
        })
    }

    /// 启动配置所指定的自动存储同步任务，并由运行时持有其生命周期。
    ///
    /// 文件监听器会在返回前建立；首次全盘协调和校验和计算在后台执行，不阻塞应用监听端口。
    ///
    /// 重复调用不会创建第二个同步任务。配置禁用同步时该方法成功返回但不启动任务。
    pub async fn start_storage_sync(&mut self) -> Result<(), CoreError> {
        if self.storage_sync.is_none() {
            self.storage_sync = self
                .infrastructure
                .start_storage_sync(self.resource_service.clone())
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
        self.resource_service.clone()
    }

    pub fn user_service(&self) -> UserService {
        self.user_service.clone()
    }

    pub fn authorization_service(&self) -> AuthorizationService {
        self.authorization_service.clone()
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
        self.resource_kind_registry.clone()
    }

    /// 返回目录类型注册表。
    pub fn directory_kind_registry(&self) -> Arc<dyn DirectoryKindRegistry> {
        self.directory_kind_registry.clone()
    }

    /// 返回启动时校验并冻结的插件浏览器静态资源。
    pub fn plugin_web_assets(&self) -> PluginWebAssets {
        self.plugin_web_assets.clone()
    }
}

fn plugin_web_assets_from_catalog(catalog: &PluginCatalog) -> Result<PluginWebAssets, CoreError> {
    let mut assets = HashMap::new();
    for plugin in catalog.plugins() {
        if plugin.web_assets().is_empty() {
            continue;
        }
        let plugin_id = plugin.manifest().plugin_id();
        if assets
            .insert(plugin_id.to_string(), plugin.web_assets().clone())
            .is_some()
        {
            return Err(CoreError::configuration(format!(
                "duplicate plugin Web root `{plugin_id}`"
            )));
        }
    }
    Ok(assets)
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
