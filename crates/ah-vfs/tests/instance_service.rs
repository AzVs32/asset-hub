//! Service contracts use an opaque driver, without any concrete backend dependency.
use common::Backend;
mod common;

use ah_vfs::application::mount_runtime::MountRuntime;
use ah_vfs::{
    application::{
        driver_catalog::DriverCatalog,
        driver_info::DriverInfo,
        dto::{InstanceInfo, InstancePage, InstanceQuery, MountFilter, RemoveInstanceOutcome},
        error::InstanceError,
        port::{DriverFuture, MountRepository, Storage},
        service::InstanceService,
    },
    domain::{
        ChildMountPolicy, DriverError, DriverInstanceRef, DriverKey, InstanceId, Mount,
        StorageCapabilities, StorageError, StorageErrorKind, VirtualPath,
    },
    infrastructure::mount_repository::SqliteMountRepository,
};
use std::{
    collections::BTreeMap,
    error::Error,
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

struct OpaqueDriver {
    info: DriverInfo,
    id: InstanceId,
    present: AtomicBool,
    removals: AtomicUsize,
    opens: AtomicUsize,
    failure: Option<StorageErrorKind>,
    creation_supported: bool,
    creations: AtomicUsize,
}

impl common::Backend for OpaqueDriver {
    fn registry(self: Arc<Self>) -> Arc<dyn ah_vfs::application::port::InstanceRegistry> {
        self
    }

    fn info(&self) -> &DriverInfo {
        &self.info
    }

    fn open(
        &self,
        context: ah_vfs::application::port::OpenContext,
    ) -> DriverFuture<'_, Arc<dyn Storage>> {
        let id = context.instance_id().clone();
        self.opens.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            if id != self.id || !self.present.load(Ordering::SeqCst) {
                return Err(DriverError::InstanceNotFound(id));
            }
            Err(StorageError::new(StorageErrorKind::UnsupportedOperation).into())
        })
    }
}
impl ah_vfs::application::port::InstanceRegistry for OpaqueDriver {
    fn create(
        &self,
        config: ah_vfs::application::dto::InstanceConfig,
    ) -> DriverFuture<'_, ah_vfs::application::dto::InstanceInfo> {
        Box::pin(async move {
            use ah_vfs::application::dto::{
                instance_config_string, validate_instance_config_fields,
            };
            self.creations.fetch_add(1, Ordering::SeqCst);
            if !self.creation_supported {
                return Err(StorageError::new(StorageErrorKind::UnsupportedOperation).into());
            }
            validate_instance_config_fields(&config, &["label", "password"])?;
            instance_config_string(&config, "label")?;
            instance_config_string(&config, "password")?;
            if let Some(kind) = self.failure {
                if kind == StorageErrorKind::OutcomeUnknown {
                    self.present.store(true, Ordering::SeqCst);
                }
                return Err(StorageError::with_source(
                    kind,
                    io::Error::other("backend-private-token"),
                )
                .into());
            }
            self.present.store(true, Ordering::SeqCst);
            Ok(InstanceInfo {
                id: self.id.clone(),
                details: BTreeMap::new(),
            })
        })
    }
    fn get(&self, id: InstanceId) -> DriverFuture<'_, Option<InstanceInfo>> {
        Box::pin(async move {
            Ok(
                (self.present.load(Ordering::SeqCst) && id == self.id).then_some(InstanceInfo {
                    id,
                    details: BTreeMap::new(),
                }),
            )
        })
    }
    fn list(&self, query: InstanceQuery) -> DriverFuture<'_, InstancePage> {
        Box::pin(async move {
            let visible = self.present.load(Ordering::SeqCst)
                && query
                    .after()
                    .is_none_or(|after| after.as_str() < self.id.as_str());
            Ok(InstancePage {
                instances: if visible {
                    vec![InstanceInfo {
                        id: self.id.clone(),
                        details: BTreeMap::new(),
                    }]
                } else {
                    vec![]
                },
                next_after: None,
            })
        })
    }
    fn remove(&self, id: InstanceId) -> DriverFuture<'_, RemoveInstanceOutcome> {
        Box::pin(async move {
            self.removals.fetch_add(1, Ordering::SeqCst);
            if let Some(kind) = self.failure {
                if kind == StorageErrorKind::OutcomeUnknown {
                    self.present.store(false, Ordering::SeqCst);
                }
                return Err(StorageError::with_source(
                    kind,
                    io::Error::other("backend-private-token"),
                )
                .into());
            }
            Ok(
                if id == self.id && self.present.swap(false, Ordering::SeqCst) {
                    RemoveInstanceOutcome::Removed
                } else {
                    RemoveInstanceOutcome::AlreadyAbsent
                },
            )
        })
    }
}

struct Fixture {
    driver: Arc<OpaqueDriver>,
    pool: sqlx::SqlitePool,
    repository: Arc<SqliteMountRepository>,
    service: InstanceService,
    instance: DriverInstanceRef,
}

impl Fixture {
    async fn new(failure: Option<StorageErrorKind>) -> TestResult<Self> {
        Self::new_with_creation(failure, false).await
    }

    async fn new_with_creation(
        failure: Option<StorageErrorKind>,
        creation_supported: bool,
    ) -> TestResult<Self> {
        let driver = Arc::new(OpaqueDriver {
            info: DriverInfo::builder()
                .key(DriverKey::parse("acme.opaque")?)
                .name("Opaque test provider".into())
                .version(semver::Version::new(1, 0, 0))
                .child_mount_policy(ChildMountPolicy::Allow)
                .maybe_instance_config_schema(creation_supported.then(|| BTreeMap::from([
            ("type".into(), "object".into()),
            ("properties".into(), serde_json::json!({"label": {"type": "string"}, "password": {"type": "string", "writeOnly": true}})),
            ("required".into(), serde_json::json!(["label", "password"])),
        ])))
                .capabilities(StorageCapabilities::default())
                .build(),
            id: InstanceId::parse("instance")?,
            present: AtomicBool::new(true),
            removals: AtomicUsize::new(0),
            opens: AtomicUsize::new(0),
            failure,
            creation_supported,
            creations: AtomicUsize::new(0),
        });
        let instance = DriverInstanceRef::new(driver.info().key().clone(), driver.id.clone());
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await?;
        let repository = Arc::new(SqliteMountRepository::from_pool(pool.clone()).await?);
        let mut catalog = DriverCatalog::new();
        common::register(&mut catalog, common::managed(driver.clone()))?;
        let service = InstanceService::new(Arc::new(MountRuntime::new(
            repository.clone(),
            Arc::new(catalog),
        )));
        Ok(Self {
            driver,
            pool,
            repository,
            service,
            instance,
        })
    }

    async fn mount(
        &self,
        path: &str,
        instance: DriverInstanceRef,
        enabled: bool,
    ) -> TestResult<Mount> {
        let mount = Mount::builder()
            .id(ah_vfs::domain::MountId::new())
            .virtual_path(VirtualPath::parse(path)?)
            .driver_instance(instance)
            .enabled(enabled)
            .build();
        self.repository.insert(mount.clone()).await?;
        Ok(mount)
    }
}

#[tokio::test]
async fn opaque_provider_is_removed_idempotently_without_opening_storage() -> TestResult {
    let f = Fixture::new(None).await?;
    assert!(f.service.get_instance(&f.instance).await?.is_some());
    assert_eq!(
        f.service
            .list_instances(f.instance.driver_key(), InstanceQuery::default())
            .await?
            .instances
            .len(),
        1
    );
    assert_eq!(
        f.service.remove_instance(&f.instance).await?,
        RemoveInstanceOutcome::Removed
    );
    assert!(f.service.get_instance(&f.instance).await?.is_none());
    assert!(
        f.service
            .list_instances(f.instance.driver_key(), InstanceQuery::default())
            .await?
            .instances
            .is_empty()
    );
    assert_eq!(
        f.service.remove_instance(&f.instance).await?,
        RemoveInstanceOutcome::AlreadyAbsent
    );
    assert_eq!(f.driver.opens.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn enabled_disabled_and_root_references_all_block_removal() -> TestResult {
    for (path, enabled) in [("/", true), ("/enabled", true), ("/disabled", false)] {
        let f = Fixture::new(None).await?;
        let mount = f.mount(path, f.instance.clone(), enabled).await?;
        let error = f
            .service
            .remove_instance(&f.instance)
            .await
            .err()
            .ok_or("in use")?;
        assert!(
            matches!(error, InstanceError::InstanceInUse { instance, mounts } if instance == f.instance && mounts == vec![mount.id()])
        );
        assert_eq!(f.driver.removals.load(Ordering::SeqCst), 0);
        assert!(f.driver.present.load(Ordering::SeqCst));
        assert_eq!(f.repository.list(MountFilter::default()).await?.len(), 1);
    }
    Ok(())
}

#[tokio::test]
async fn all_aliases_are_reported_and_other_instance_identities_do_not_block() -> TestResult {
    let f = Fixture::new(None).await?;
    let last = f.mount("/z", f.instance.clone(), true).await?;
    let first = f.mount("/a", f.instance.clone(), false).await?;
    let error = f
        .service
        .remove_instance(&f.instance)
        .await
        .err()
        .ok_or("aliases")?;
    assert!(
        matches!(error, InstanceError::InstanceInUse { mounts, .. } if mounts == vec![first.id(), last.id()])
    );
    f.repository.remove(first.id()).await?;
    f.repository.remove(last.id()).await?;
    f.mount(
        "/other-driver",
        DriverInstanceRef::new(DriverKey::parse("another.provider")?, f.driver.id.clone()),
        true,
    )
    .await?;
    f.mount(
        "/other-instance",
        DriverInstanceRef::new(f.instance.driver_key().clone(), InstanceId::parse("other")?),
        false,
    )
    .await?;
    assert_eq!(
        f.service.remove_instance(&f.instance).await?,
        RemoveInstanceOutcome::Removed
    );
    assert_eq!(f.repository.list(MountFilter::default()).await?.len(), 2);
    Ok(())
}

#[tokio::test]
async fn dangling_references_are_not_hidden_by_an_already_absent_driver_record() -> TestResult {
    let f = Fixture::new(None).await?;
    f.driver.present.store(false, Ordering::SeqCst);
    f.mount("/", f.instance.clone(), false).await?;
    assert!(matches!(
        f.service.remove_instance(&f.instance).await,
        Err(InstanceError::InstanceInUse { .. })
    ));
    assert_eq!(f.driver.removals.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn repository_failures_stop_removal_before_the_driver_is_called() -> TestResult {
    let f = Fixture::new(None).await?;
    f.pool.close().await;
    assert!(matches!(
        f.service.remove_instance(&f.instance).await,
        Err(InstanceError::Repository(_))
    ));
    assert_eq!(f.driver.removals.load(Ordering::SeqCst), 0);
    assert!(f.driver.present.load(Ordering::SeqCst));
    Ok(())
}

#[tokio::test]
async fn unknown_drivers_fail_through_the_same_public_service() -> TestResult {
    let f = Fixture::new(None).await?;
    let missing = DriverInstanceRef::new(DriverKey::parse("missing")?, f.driver.id.clone());
    assert!(matches!(
        f.service.instance_config_schema(missing.driver_key()),
        Err(InstanceError::DriverNotRegistered(_))
    ));
    assert!(matches!(
        f.service
            .create_instance(missing.driver_key(), BTreeMap::new().into())
            .await,
        Err(InstanceError::DriverNotRegistered(_))
    ));
    assert!(matches!(
        f.service.get_instance(&missing).await,
        Err(InstanceError::DriverNotRegistered(_))
    ));
    assert!(matches!(
        f.service
            .list_instances(missing.driver_key(), InstanceQuery::default())
            .await,
        Err(InstanceError::DriverNotRegistered(_))
    ));
    assert!(matches!(
        f.service.remove_instance(&missing).await,
        Err(InstanceError::DriverNotRegistered(_))
    ));
    assert_eq!(f.driver.removals.load(Ordering::SeqCst), 0);
    Ok(())
}

fn opaque_config() -> ah_vfs::application::dto::InstanceConfig {
    BTreeMap::from([
        ("label".into(), "demo".into()),
        ("password".into(), "private-input-value".into()),
    ])
    .into()
}

#[tokio::test]
async fn creation_support_is_explicit_and_checked_before_invoking_the_driver() -> TestResult {
    let f = Fixture::new(None).await?;
    assert!(
        f.service
            .instance_config_schema(f.instance.driver_key())?
            .is_none()
    );
    let error = f
        .service
        .create_instance(f.instance.driver_key(), BTreeMap::new().into())
        .await
        .err()
        .ok_or("unsupported")?;
    assert_eq!(
        error.storage_error().ok_or("domain error")?.kind(),
        StorageErrorKind::UnsupportedOperation
    );
    assert_eq!(f.driver.creations.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn unknown_provider_creation_is_forwarded_without_host_field_or_type_knowledge() -> TestResult
{
    let f = Fixture::new_with_creation(None, true).await?;
    f.driver.present.store(false, Ordering::SeqCst);
    let schema = f
        .service
        .instance_config_schema(f.instance.driver_key())?
        .ok_or("schema")?;
    assert_eq!(schema["properties"]["password"]["writeOnly"], true);
    let created = f
        .service
        .create_instance(f.instance.driver_key(), opaque_config())
        .await?;
    assert_eq!(
        f.service.get_instance(&f.instance).await?,
        Some(created.clone())
    );
    assert_eq!(
        f.service
            .list_instances(f.instance.driver_key(), InstanceQuery::default())
            .await?
            .instances,
        vec![created.clone()]
    );
    assert!(!format!("{created:?}").contains("private-input-value"));
    assert_eq!(f.driver.creations.load(Ordering::SeqCst), 1);
    assert_eq!(f.driver.opens.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn creation_failures_are_not_retried_or_blindly_compensated() -> TestResult {
    for kind in [
        StorageErrorKind::Unavailable,
        StorageErrorKind::OutcomeUnknown,
    ] {
        let f = Fixture::new_with_creation(Some(kind), true).await?;
        f.driver.present.store(false, Ordering::SeqCst);
        let error = f
            .service
            .create_instance(f.instance.driver_key(), opaque_config())
            .await
            .err()
            .ok_or("failure")?;
        let storage = error.storage_error().ok_or("domain error")?;
        assert_eq!(storage.kind(), kind);
        assert!(storage.source().is_some());
        assert!(!error.to_string().contains("backend-private-token"));
        assert_eq!(f.driver.creations.load(Ordering::SeqCst), 1);
        assert_eq!(f.driver.removals.load(Ordering::SeqCst), 0);
        assert_eq!(f.driver.opens.load(Ordering::SeqCst), 0);
        assert_eq!(
            f.service.get_instance(&f.instance).await?.is_some(),
            kind == StorageErrorKind::OutcomeUnknown
        );
    }
    Ok(())
}

#[tokio::test]
async fn unsupported_and_failed_removals_preserve_domain_categories_and_sources() -> TestResult {
    for kind in [
        StorageErrorKind::UnsupportedOperation,
        StorageErrorKind::Unavailable,
        StorageErrorKind::PermissionDenied,
    ] {
        let f = Fixture::new(Some(kind)).await?;
        let error = f
            .service
            .remove_instance(&f.instance)
            .await
            .err()
            .ok_or("failure")?;
        let backend = error.storage_error().ok_or("backend error")?;
        assert_eq!(backend.kind(), kind);
        assert!(backend.source().is_some());
        assert!(!error.to_string().contains("backend-private-token"));
        assert!(f.driver.present.load(Ordering::SeqCst));
    }
    Ok(())
}

#[tokio::test]
async fn an_uncertain_remote_outcome_is_not_reported_as_confirmed_absence() -> TestResult {
    let f = Fixture::new(Some(StorageErrorKind::OutcomeUnknown)).await?;
    let error = f
        .service
        .remove_instance(&f.instance)
        .await
        .err()
        .ok_or("unknown outcome")?;
    assert_eq!(
        error.storage_error().ok_or("backend")?.kind(),
        StorageErrorKind::OutcomeUnknown
    );
    assert!(f.service.get_instance(&f.instance).await?.is_none());
    Ok(())
}
