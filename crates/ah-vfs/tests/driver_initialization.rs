use ah_vfs::{
    application::{
        driver_catalog::DriverCatalog, driver_info::DriverInfo, dto::*, error::DriverCatalogError,
        port::*,
    },
    domain::*,
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
impl InstanceRegistry for Records {
    fn create(&self, config: InstanceConfig) -> DriverFuture<'_, InstanceInfo> {
        Box::pin(async move {
            assert!(!config.contains_key("database_url"));
            *self.config.lock().map_err(|_| unsupported())? = Some(config);
            Ok(InstanceInfo {
                id: InstanceId::parse("demo").map_err(|_| unsupported())?,
                details: BTreeMap::new(),
            })
        })
    }
    fn get(&self, _: InstanceId) -> DriverFuture<'_, Option<InstanceInfo>> {
        Box::pin(async { Ok(None) })
    }
    fn list(&self, _: InstanceQuery) -> DriverFuture<'_, InstancePage> {
        Box::pin(async {
            Ok(InstancePage {
                instances: vec![],
                next_after: None,
            })
        })
    }
    fn remove(&self, _: InstanceId) -> DriverFuture<'_, RemoveInstanceOutcome> {
        Box::pin(async { Ok(RemoveInstanceOutcome::AlreadyAbsent) })
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
                .name("Self-managed plugin".into())
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
    fn instances(&self) -> InstanceManagement {
        InstanceManagement::SelfManaged(self.registry.clone())
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
    selected
        .instances()
        .registry()?
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
    fn instances(&self) -> InstanceManagement {
        InstanceManagement::SelfManaged(self.registry.clone())
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
