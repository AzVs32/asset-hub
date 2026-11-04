use ah_vfs::{
    application::{
        driver_catalog::DriverCatalog, driver_info::DriverInfo, dto::*, error::DriverCatalogError,
        port::*,
    },
    domain::{
        DriverError, DriverKey,
        namespace::ChildMountPolicy,
        storage::{
            FileWriteSupport, StorageCapabilities, StorageCapabilitiesError, StorageError,
            StorageErrorKind,
        },
    },
};
use std::{
    collections::BTreeMap,
    future::{Future, poll_fn},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    task::Poll,
};
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
fn unsupported() -> DriverError {
    StorageError::new(StorageErrorKind::UnsupportedOperation).into()
}

#[derive(Default)]
struct Records {
    config: Mutex<Option<InstanceConfig>>,
}
impl InstanceProvisioner for Records {
    fn prepare(&self, config: InstanceConfig) -> DriverFuture<'_, PreparedConfig> {
        Box::pin(async move {
            assert!(!config.contains_key("database_url"));
            *self.config.lock().map_err(|_| unsupported())? = Some(config.clone());
            Ok(PreparedConfig::new(config, BTreeMap::new()))
        })
    }
}
struct Plugin {
    info: DriverInfo,
    calls: AtomicUsize,
    settings: Mutex<Option<DriverSettings>>,
    registry: Arc<Records>,
    fail: bool,
    gate: Option<Arc<tokio::sync::Notify>>,
}
impl Plugin {
    fn new(fail: bool, gate: Option<Arc<tokio::sync::Notify>>) -> TestResult<Self> {
        Ok(Self {
            info: DriverInfo::builder()
                .key(DriverKey::parse("own.database")?)
                .name("Host-managed plugin".into())
                .version(semver::Version::new(1, 0, 0))
                .child_mount_policy(ChildMountPolicy::Allow)
                .capabilities(StorageCapabilities::default())
                .settings_schema(BTreeMap::from([(
                    "properties".into(),
                    serde_json::json!({"database_url":{"type":"string","x-secret":true}}),
                )]))
                .instance_config_schema(BTreeMap::from([(
                    "properties".into(),
                    serde_json::json!({"label":{"type":"string"}}),
                )]))
                .build(),
            calls: AtomicUsize::new(0),
            settings: Mutex::new(None),
            registry: Arc::new(Records::default()),
            fail,
            gate,
        })
    }
}
impl Driver for Plugin {
    fn info(&self) -> &DriverInfo {
        &self.info
    }
    fn provisioner(&self) -> Arc<dyn InstanceProvisioner> {
        self.registry.clone()
    }
    fn initialize(&self, settings: DriverSettings) -> DriverFuture<'_, ()> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if let Some(gate) = &self.gate {
                gate.notified().await;
            }
            if self.fail {
                return Err(StorageError::with_source(
                    StorageErrorKind::Unavailable,
                    std::io::Error::other("private-database-password"),
                )
                .into());
            }
            if settings
                .get("database_url")
                .and_then(serde_json::Value::as_str)
                .is_none()
            {
                return Err(unsupported());
            }
            *self.settings.lock().map_err(|_| unsupported())? = Some(settings);
            Ok(())
        })
    }
    fn open(&self, _: OpenContext) -> DriverFuture<'_, Arc<dyn Storage>> {
        Box::pin(async { Err(unsupported()) })
    }
}
fn settings() -> DriverSettings {
    DriverSettings::from([(
        "database_url".into(),
        "postgres://user:private-database-password@db/plugins".into(),
    )])
}

#[tokio::test]
async fn settings_initialize_before_registration_and_stay_separate_from_instance_parameters()
-> TestResult {
    let driver = Arc::new(Plugin::new(false, None)?);
    let mut catalog = DriverCatalog::new();
    catalog.register(driver.clone(), settings()).await?;
    assert_eq!(driver.calls.load(Ordering::SeqCst), 1);
    let selected = catalog.get(driver.info.key()).ok_or("driver")?;
    let records=Arc::new(ah_vfs::infrastructure::instance_record_repository::SqliteInstanceRecordRepository::from_pool(sqlx::sqlite::SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await?).await?);
    ah_vfs::application::host_instance_registry::HostInstanceRegistry::new(
        selected.info(),
        selected.provisioner(),
        records,
        None,
    )
    .create(InstanceConfig::from([(
        "label".into(),
        "per-instance".into(),
    )]))
    .await?;
    assert!(
        driver
            .registry
            .config
            .lock()
            .map_err(|_| "lock")?
            .as_ref()
            .is_some_and(|config| config.get("label") == Some(&"per-instance".into()))
    );
    assert!(
        driver
            .settings
            .lock()
            .map_err(|_| "lock")?
            .as_ref()
            .is_some_and(|config| config.contains_key("database_url"))
    );
    let first = driver.info.settings_schema().ok_or("schema")?;
    let second = driver.info.settings_schema().ok_or("schema")?;
    assert!(std::ptr::eq(first, second));
    assert!(!format!("{:?}", settings()).contains("private-database-password"));
    assert!(!format!("{catalog:?}").contains("private-database-password"));
    Ok(())
}

#[tokio::test]
async fn duplicates_are_rejected_before_initializing_the_replacement() -> TestResult {
    let mut catalog = DriverCatalog::new();
    let original = Arc::new(Plugin::new(false, None)?);
    let duplicate = Arc::new(Plugin::new(false, None)?);
    catalog.register(original.clone(), settings()).await?;
    assert!(matches!(
        catalog.register(duplicate.clone(), settings()).await,
        Err(DriverCatalogError::AlreadyRegistered(_))
    ));
    assert_eq!(original.calls.load(Ordering::SeqCst), 1);
    assert_eq!(duplicate.calls.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn invalid_capabilities_fail_before_initialization_and_do_not_reserve_the_key() -> TestResult
{
    let declarations = [
        StorageCapabilities {
            write_file: FileWriteSupport {
                atomic_publish: true,
                ..Default::default()
            },
            ..Default::default()
        },
        StorageCapabilities {
            persistent_empty_dirs: true,
            ..Default::default()
        },
        StorageCapabilities {
            create_dir: true,
            ..Default::default()
        },
    ];
    for capabilities in declarations {
        let mut plugin = Plugin::new(false, None)?;
        plugin.info = DriverInfo::builder()
            .key(DriverKey::parse("own.database")?)
            .name("Invalid declaration".into())
            .version(semver::Version::new(1, 0, 0))
            .child_mount_policy(ChildMountPolicy::Allow)
            .capabilities(capabilities)
            .build();
        let plugin = Arc::new(plugin);
        let mut catalog = DriverCatalog::new();
        let error = catalog
            .register(plugin.clone(), settings())
            .await
            .err()
            .ok_or("expected invalid declaration")?;
        assert!(
            matches!(&error, DriverCatalogError::InvalidDeclaration(error) if error.storage_error().is_some_and(|e| e.kind() == StorageErrorKind::InvalidData))
        );
        let source = std::error::Error::source(&error)
            .and_then(std::error::Error::source)
            .and_then(|e| e.downcast_ref::<StorageCapabilitiesError>());
        assert_eq!(source.copied(), capabilities.validate().err());
        assert_eq!(plugin.calls.load(Ordering::SeqCst), 0);
        assert!(catalog.list().is_empty());
        let valid = Arc::new(Plugin::new(false, None)?);
        catalog.register(valid, settings()).await?;
        assert!(catalog.get(plugin.info.key()).is_some());
    }
    Ok(())
}

#[tokio::test]
async fn failed_initialization_does_not_register_or_disclose_credentials() -> TestResult {
    let mut catalog = DriverCatalog::new();
    let driver = Arc::new(Plugin::new(true, None)?);
    let error = catalog
        .register(driver.clone(), settings())
        .await
        .err()
        .ok_or("failure")?;
    assert!(matches!(error, DriverCatalogError::Initialization(_)));
    assert!(!error.to_string().contains("private-database-password"));
    assert!(!format!("{error:?}").contains("private-database-password"));
    assert!(std::error::Error::source(&error).is_some());
    assert!(catalog.list().is_empty());
    assert_eq!(driver.calls.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn cancelled_initialization_never_publishes_a_partial_registration() -> TestResult {
    let mut catalog = DriverCatalog::new();
    let driver = Arc::new(Plugin::new(
        false,
        Some(Arc::new(tokio::sync::Notify::new())),
    )?);
    let mut registration = Box::pin(catalog.register(driver.clone(), settings()));
    assert!(
        poll_fn(|cx| Poll::Ready(registration.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    drop(registration);
    assert!(catalog.list().is_empty());
    assert!(driver.settings.lock().map_err(|_| "lock")?.is_none());
    Ok(())
}

struct DefaultPlugin {
    info: DriverInfo,
    registry: Arc<Records>,
}
impl Driver for DefaultPlugin {
    fn info(&self) -> &DriverInfo {
        &self.info
    }
    fn provisioner(&self) -> Arc<dyn InstanceProvisioner> {
        self.registry.clone()
    }
    fn open(&self, _: OpenContext) -> DriverFuture<'_, Arc<dyn Storage>> {
        Box::pin(async { Err(unsupported()) })
    }
}
#[tokio::test]
async fn default_initialization_accepts_only_empty_settings_without_a_schema() -> TestResult {
    let info = DriverInfo::builder()
        .key(DriverKey::parse("assembled")?)
        .name("Already assembled".into())
        .version(semver::Version::new(1, 0, 0))
        .child_mount_policy(ChildMountPolicy::Allow)
        .capabilities(StorageCapabilities::default())
        .build();
    let driver = DefaultPlugin {
        info,
        registry: Arc::new(Records::default()),
    };
    driver.initialize(DriverSettings::new()).await?;
    assert!(
        matches!(driver.initialize(settings()).await,Err(DriverError::Storage(error)) if error.kind()==StorageErrorKind::UnsupportedOperation)
    );
    let plugin = DefaultPlugin {
        info: Plugin::new(false, None)?.info,
        registry: Arc::new(Records::default()),
    };
    assert!(plugin.initialize(DriverSettings::new()).await.is_err());
    Ok(())
}
