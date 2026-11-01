use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use ah_vfs::{
    application::{
        driver_catalog::DriverCatalog,
        driver_info::DriverInfo,
        dto::*,
        host_instance_registry::HostInstanceRegistry,
        mount_runtime::MountRuntime,
        port::*,
        service::{EntryService, InstanceService, MountService},
    },
    domain::*,
    infrastructure::{
        instance_record_repository::SqliteInstanceRecordRepository,
        mount_repository::SqliteMountRepository,
    },
};
use std::{
    collections::BTreeMap,
    num::{NonZeroU32, NonZeroUsize},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
fn invalid() -> DriverError {
    StorageError::new(StorageErrorKind::InvalidData).into()
}

// Real authenticated encryption for isolated fixtures only. A production host
// supplies its own persisted key/nonce lifecycle through the SecretCipher port.
struct TestCipher {
    cipher: Aes256Gcm,
    nonce: AtomicU64,
}
impl TestCipher {
    fn new() -> Self {
        Self {
            cipher: Aes256Gcm::new(&[19; 32].into()),
            nonce: AtomicU64::new(1),
        }
    }
}
fn aad(context: &SecretContext) -> Vec<u8> {
    serde_json::to_vec(&(
        context.driver_key().as_str(),
        context.instance_id().as_str(),
        context.pointer(),
    ))
    .unwrap_or_default()
}
impl SecretCipher for TestCipher {
    fn encrypt<'a>(
        &'a self,
        plaintext: &'a [u8],
        context: SecretContext,
    ) -> DriverFuture<'a, Vec<u8>> {
        Box::pin(async move {
            let mut nonce = [0; 12];
            nonce[4..].copy_from_slice(&self.nonce.fetch_add(1, Ordering::SeqCst).to_be_bytes());
            let mut output = nonce.to_vec();
            output.extend(
                self.cipher
                    .encrypt(
                        Nonce::from_slice(&nonce),
                        Payload {
                            msg: plaintext,
                            aad: &aad(&context),
                        },
                    )
                    .map_err(|_| invalid())?,
            );
            Ok(output)
        })
    }
    fn decrypt<'a>(
        &'a self,
        ciphertext: &'a [u8],
        context: SecretContext,
    ) -> DriverFuture<'a, Vec<u8>> {
        Box::pin(async move {
            if ciphertext.len() < 12 {
                return Err(invalid());
            }
            self.cipher
                .decrypt(
                    Nonce::from_slice(&ciphertext[..12]),
                    Payload {
                        msg: &ciphertext[12..],
                        aad: &aad(&context),
                    },
                )
                .map_err(|_| invalid())
        })
    }
}
#[derive(Default)]
struct Provisioner {
    prepares: AtomicUsize,
    migrations: AtomicUsize,
    fail_migration: AtomicBool,
}
impl InstanceProvisioner for Provisioner {
    fn prepare(&self, config: InstanceConfig) -> DriverFuture<'_, PreparedConfig> {
        Box::pin(async move {
            self.prepares.fetch_add(1, Ordering::SeqCst);
            validate_instance_config_fields(
                &config,
                &["endpoint", "label", "password", "auth", "tokens"],
            )?;
            instance_config_string(&config, "endpoint")?;
            let label = instance_config_string(&config, "label")?.trim().to_owned();
            let password = instance_config_string(&config, "password")?.to_owned();
            let mut fields = config.into_object();
            fields.insert("label".into(), label.clone().into());
            Ok(PreparedConfig::new(
                fields.into(),
                BTreeMap::from([
                    ("label".into(), label.into()),
                    ("password".into(), password.clone().into()),
                    (
                        "url".into(),
                        format!("https://user:{password}@example").into(),
                    ),
                ]),
            ))
        })
    }
    fn migrate_config(
        &self,
        _from: u32,
        config: InstanceConfig,
    ) -> DriverFuture<'_, InstanceConfig> {
        Box::pin(async move {
            self.migrations.fetch_add(1, Ordering::SeqCst);
            if self.fail_migration.load(Ordering::SeqCst) {
                return Err(StorageError::with_source(
                    StorageErrorKind::Backend,
                    std::io::Error::other("secret-migration-diagnostic"),
                )
                .into());
            }
            Ok(config)
        })
    }
}

struct AsyncProvisioner {
    validation: Provisioner,
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
    migrations: AtomicUsize,
    result: Result<InstanceConfig, StorageErrorKind>,
    prepare_error: Option<StorageErrorKind>,
}
impl AsyncProvisioner {
    fn new(result: Result<InstanceConfig, StorageErrorKind>) -> Self {
        Self {
            validation: Provisioner::default(),
            entered: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
            migrations: AtomicUsize::new(0),
            result,
            prepare_error: None,
        }
    }
}
impl InstanceProvisioner for AsyncProvisioner {
    fn prepare(&self, config: InstanceConfig) -> DriverFuture<'_, PreparedConfig> {
        if let Some(kind) = self.prepare_error {
            return Box::pin(async move { Err(StorageError::new(kind).into()) });
        }
        self.validation.prepare(config)
    }

    fn migrate_config(
        &self,
        _from: u32,
        _config: InstanceConfig,
    ) -> DriverFuture<'_, InstanceConfig> {
        Box::pin(async move {
            self.migrations.fetch_add(1, Ordering::SeqCst);
            self.entered.notify_one();
            self.release.notified().await;
            self.result.clone().map_err(|kind| {
                StorageError::with_source(kind, std::io::Error::other("private-plugin-diagnostic"))
                    .into()
            })
        })
    }
}

fn schema(secret: bool) -> InstanceConfigSchema {
    BTreeMap::from([
        ("type".into(), "object".into()),
        (
            "properties".into(),
            serde_json::json!({"endpoint":{"type":"string","x-immutable":true},"label":{"type":"string"},"password":{"type":"string","x-secret":secret},"auth":{"type":"object","properties":{"token":{"type":"string","x-secret":secret}}},"tokens":{"type":"array","items":{"type":"string","x-secret":secret}}}),
        ),
    ])
}
fn config() -> InstanceConfig {
    InstanceConfig::from([
        ("endpoint".into(), "stable-endpoint".into()),
        ("label".into(), " first ".into()),
        ("password".into(), "password-value-private".into()),
        (
            "auth".into(),
            serde_json::json!({"token":"nested-token-private"}),
        ),
        ("tokens".into(), serde_json::json!(["array-token-private"])),
    ])
}
fn info(version: u32, secret: bool) -> TestResult<DriverInfo> {
    Ok(DriverInfo::builder()
        .key(DriverKey::parse("host.test")?)
        .name("Host test".into())
        .version(semver::Version::new(1, 0, 0))
        .child_mount_policy(ChildMountPolicy::Allow)
        .capabilities(StorageCapabilities::default())
        .instance_config_schema(schema(secret))
        .instance_config_version(NonZeroU32::new(version).ok_or("version")?)
        .build())
}
struct HostedDriver {
    info: DriverInfo,
    provisioner: Arc<Provisioner>,
    opens: AtomicUsize,
    seen: Mutex<Option<InstanceConfig>>,
}
struct EmptyStorage;
impl Storage for EmptyStorage {
    fn capabilities(&self) -> StorageCapabilities {
        StorageCapabilities::default()
    }
    fn chunk_size(&self) -> NonZeroUsize {
        NonZeroUsize::MIN
    }
}
impl Driver for HostedDriver {
    fn info(&self) -> &DriverInfo {
        &self.info
    }
    fn provisioner(&self) -> Arc<dyn InstanceProvisioner> {
        self.provisioner.clone()
    }
    fn open(&self, context: OpenContext) -> DriverFuture<'_, Arc<dyn Storage>> {
        Box::pin(async move {
            self.opens.fetch_add(1, Ordering::SeqCst);
            *self.seen.lock().map_err(|_| invalid())? = Some(context.config().clone());
            Ok(Arc::new(EmptyStorage) as Arc<dyn Storage>)
        })
    }
}
struct Fixture {
    pool: sqlx::SqlitePool,
    records: Arc<SqliteInstanceRecordRepository>,
    driver: Arc<HostedDriver>,
    runtime: Arc<MountRuntime>,
    cipher: Option<Arc<dyn SecretCipher>>,
}
impl Fixture {
    async fn new(secret: bool, cipher: bool) -> TestResult<Self> {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await?;
        let records = Arc::new(SqliteInstanceRecordRepository::from_pool(pool.clone()).await?);
        let driver = Arc::new(HostedDriver {
            info: info(1, secret)?,
            provisioner: Arc::new(Provisioner::default()),
            opens: AtomicUsize::new(0),
            seen: Mutex::new(None),
        });
        let mut catalog = DriverCatalog::new();
        catalog.register(driver.clone(), Default::default()).await?;
        let mounts = Arc::new(SqliteMountRepository::from_pool(pool.clone()).await?);
        let cipher = cipher.then(|| Arc::new(TestCipher::new()) as Arc<dyn SecretCipher>);
        let runtime = Arc::new(MountRuntime::new(
            mounts,
            Arc::new(catalog),
            records.clone(),
            cipher.clone(),
        ));
        Ok(Self {
            pool,
            records,
            driver,
            runtime,
            cipher,
        })
    }
    fn reference(&self, id: InstanceId) -> DriverInstanceRef {
        DriverInstanceRef::new(self.driver.info.key().clone(), id)
    }
    fn service(&self) -> InstanceService {
        InstanceService::new(self.runtime.clone())
    }
}

#[tokio::test]
async fn host_lifecycle_encrypts_nested_secrets_and_passes_decrypted_open_context() -> TestResult {
    let f = Fixture::new(true, true).await?;
    let service = f.service();
    let created = service
        .create_instance(f.driver.info.key(), config())
        .await?;
    let reference = f.reference(created.id.clone());
    let public = service.get_instance(&reference).await?.ok_or("record")?;
    assert_eq!(public.details["label"], "first");
    let record = f.records.get(reference.clone()).await?.ok_or("record")?;
    let stored: String = sqlx::query_scalar("SELECT config_json FROM vfs_instance_records")
        .fetch_one(&f.pool)
        .await?;
    for secret in [
        "password-value-private",
        "nested-token-private",
        "array-token-private",
    ] {
        for output in [
            stored.clone(),
            format!("{record:?}"),
            format!("{public:?}"),
            format!("{:?}", config()),
            format!(
                "{:?}",
                service
                    .list_instances(f.driver.info.key(), InstanceQuery::default())
                    .await?
            ),
        ] {
            assert!(!output.contains(secret));
        }
    }
    assert_eq!(f.driver.opens.load(Ordering::SeqCst), 0);
    let mounts = MountService::new(f.runtime.clone());
    let mounted = mounts
        .mount(MountRequest {
            virtual_path: VirtualPath::root(),
            driver_instance: reference.clone(),
        })
        .await?;
    assert_eq!(
        f.driver
            .seen
            .lock()
            .map_err(|_| "lock")?
            .as_ref()
            .ok_or("seen")?
            .get("password"),
        Some(&"password-value-private".into())
    );
    let entries = EntryService::new(f.runtime.clone());
    entries.capabilities(&VirtualPath::root()).await?;
    let mut fields = config().into_object();
    fields.insert("label".into(), "second".into());
    service.update_instance(&reference, fields.into()).await?;
    let changed = f.records.get(reference.clone()).await?.ok_or("record")?;
    assert_eq!(changed.config_version(), 1);
    assert_eq!(changed.revision(), 2);
    assert!(changed.updated_at() > record.updated_at());
    entries.capabilities(&VirtualPath::root()).await?;
    assert_eq!(f.driver.opens.load(Ordering::SeqCst), 2);
    let mut fields = config().into_object();
    fields.insert("endpoint".into(), "changed".into());
    assert!(
        service
            .update_instance(&reference, fields.into())
            .await
            .is_err()
    );
    mounts.disable(mounted.id).await?;
    assert!(matches!(
        service.remove_instance(&reference).await,
        Err(ah_vfs::application::error::InstanceError::InstanceInUse { .. })
    ));
    // The required root cannot be unmounted; remove its definition only in this isolated fixture.
    sqlx::query("DELETE FROM vfs_mounts")
        .execute(&f.pool)
        .await?;
    f.runtime.invalidate();
    assert_eq!(
        service.remove_instance(&reference).await?,
        RemoveInstanceOutcome::Removed
    );
    assert_eq!(
        service.remove_instance(&reference).await?,
        RemoveInstanceOutcome::AlreadyAbsent
    );
    Ok(())
}

#[tokio::test]
async fn secret_schema_without_cipher_fails_before_prepare_or_persistence() -> TestResult {
    let f = Fixture::new(true, false).await?;
    let error = f
        .service()
        .create_instance(f.driver.info.key(), config())
        .await
        .err()
        .ok_or("failure")?;
    assert_eq!(
        error.storage_error().ok_or("category")?.kind(),
        StorageErrorKind::UnsupportedOperation
    );
    assert_eq!(f.driver.provisioner.prepares.load(Ordering::SeqCst), 0);
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM vfs_instance_records")
        .fetch_one(&f.pool)
        .await?;
    assert_eq!(count, 0);
    assert!(!format!("{error:?}").contains("password-value-private"));
    Ok(())
}

#[tokio::test]
async fn schema_migration_is_per_instance_and_independent_of_record_revision() -> TestResult {
    let f = Fixture::new(true, true).await?;
    let service = f.service();
    let first = service
        .create_instance(f.driver.info.key(), config())
        .await?;
    let second = service
        .create_instance(f.driver.info.key(), config())
        .await?;
    let reference = f.reference(first.id.clone());
    for _ in 0..3 {
        service.update_instance(&reference, config()).await?;
    }
    let key_cipher = f.cipher.clone().ok_or("cipher")?;
    // Fixture keys match; decrypt does not consume the nonce counter.
    let provisioner = Arc::new(Provisioner::default());
    let registry = HostInstanceRegistry::new(
        &info(2, true)?,
        provisioner.clone(),
        f.records.clone(),
        Some(key_cipher),
    );
    provisioner.fail_migration.store(true, Ordering::SeqCst);
    let error = registry
        .open_context(first.id.clone())
        .await
        .err()
        .ok_or("migration failure")?;
    assert!(
        matches!(error,DriverError::Storage(error) if error.kind()==StorageErrorKind::Backend && !format!("{error:?}").contains("secret-migration-diagnostic"))
    );
    assert_eq!(
        f.records
            .get(reference.clone())
            .await?
            .ok_or("record")?
            .config_version(),
        1
    );
    provisioner.fail_migration.store(false, Ordering::SeqCst);
    let context = registry.open_context(second.id.clone()).await?;
    assert!(!context.config().is_empty());
    assert_eq!(
        f.records
            .get(f.reference(second.id))
            .await?
            .ok_or("record")?
            .config_version(),
        2
    );
    registry.open_context(first.id).await?;
    let migrated = f.records.get(reference).await?.ok_or("record")?;
    assert_eq!(migrated.config_version(), 2);
    assert_eq!(migrated.revision(), 5);
    Ok(())
}

#[tokio::test]
async fn cancelled_async_migration_leaves_records_unchanged_and_can_be_retried() -> TestResult {
    let f = Fixture::new(true, true).await?;
    let created = f
        .service()
        .create_instance(f.driver.info.key(), config())
        .await?;
    let reference = f.reference(created.id.clone());
    let before = f.records.get(reference.clone()).await?.ok_or("record")?;
    let mut fields = config().into_object();
    fields.insert("label".into(), " migrated ".into());
    let provisioner = Arc::new(AsyncProvisioner::new(Ok(fields.into())));
    let registry = HostInstanceRegistry::new(
        &info(2, true)?,
        provisioner.clone(),
        f.records.clone(),
        f.cipher.clone(),
    );

    let mut opening = Box::pin(registry.open_context(created.id.clone()));
    tokio::select! {
        result = &mut opening => return Err(format!("migration did not wait: {result:?}").into()),
        () = provisioner.entered.notified() => {}
    }
    // Other record reads remain available while plugin execution is suspended.
    assert!(registry.get(created.id.clone()).await?.is_some());
    assert_eq!(provisioner.validation.prepares.load(Ordering::SeqCst), 0);
    drop(opening);
    let cancelled = f.records.get(reference.clone()).await?.ok_or("record")?;
    assert_eq!(cancelled.config_json(), before.config_json());
    assert_eq!(cancelled.config_version(), before.config_version());
    assert_eq!(cancelled.revision(), before.revision());
    assert_eq!(cancelled.updated_at(), before.updated_at());

    provisioner.release.notify_one();
    let context = registry.open_context(created.id).await?;
    assert_eq!(context.config()["label"], "migrated");
    let migrated = f.records.get(reference).await?.ok_or("record")?;
    assert_eq!(migrated.config_version(), 2);
    assert_eq!(migrated.revision(), before.revision() + 1);
    assert_eq!(provisioner.migrations.load(Ordering::SeqCst), 2);
    assert_eq!(provisioner.validation.prepares.load(Ordering::SeqCst), 1);
    assert_eq!(f.driver.opens.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn async_migration_preserves_runtime_errors_and_rejects_invalid_results() -> TestResult {
    let f = Fixture::new(true, true).await?;
    let created = f
        .service()
        .create_instance(f.driver.info.key(), config())
        .await?;
    let reference = f.reference(created.id.clone());
    let before = f.records.get(reference.clone()).await?.ok_or("record")?;
    for kind in [
        StorageErrorKind::Unavailable,
        StorageErrorKind::TimedOut,
        StorageErrorKind::Cancelled,
        StorageErrorKind::OutcomeUnknown,
        StorageErrorKind::StaleHandle,
        StorageErrorKind::Backend,
    ] {
        for fail_prepare in [false, true] {
            let mut provisioner = AsyncProvisioner::new(if fail_prepare {
                Ok(config())
            } else {
                Err(kind)
            });
            provisioner.prepare_error = fail_prepare.then_some(kind);
            provisioner.release.notify_one();
            let registry = HostInstanceRegistry::new(
                &info(2, true)?,
                Arc::new(provisioner),
                f.records.clone(),
                f.cipher.clone(),
            );
            let error = registry
                .open_context(created.id.clone())
                .await
                .err()
                .ok_or("migration failure")?;
            assert_eq!(error.storage_error().ok_or("category")?.kind(), kind);
            assert!(!format!("{error:?}").contains("private-plugin-diagnostic"));
            let unchanged = f.records.get(reference.clone()).await?.ok_or("record")?;
            assert_eq!(unchanged.config_json(), before.config_json());
            assert_eq!(unchanged.config_version(), before.config_version());
            assert_eq!(unchanged.revision(), before.revision());
        }
    }

    let mut fields = config().into_object();
    fields.insert("label".into(), 42.into());
    let provisioner = AsyncProvisioner::new(Ok(fields.into()));
    provisioner.release.notify_one();
    let registry = HostInstanceRegistry::new(
        &info(2, true)?,
        Arc::new(provisioner),
        f.records.clone(),
        f.cipher.clone(),
    );
    let error = registry
        .open_context(created.id)
        .await
        .err()
        .ok_or("invalid migrated config")?;
    assert_eq!(
        error.storage_error().ok_or("category")?.kind(),
        StorageErrorKind::InvalidData
    );
    let unchanged = f.records.get(reference).await?.ok_or("record")?;
    assert_eq!(unchanged.config_json(), before.config_json());
    assert_eq!(unchanged.config_version(), before.config_version());
    assert_eq!(unchanged.revision(), before.revision());
    Ok(())
}

#[tokio::test]
async fn async_migration_cannot_overwrite_an_update_made_while_waiting() -> TestResult {
    let f = Fixture::new(true, true).await?;
    let created = f
        .service()
        .create_instance(f.driver.info.key(), config())
        .await?;
    let reference = f.reference(created.id.clone());
    let provisioner = Arc::new(AsyncProvisioner::new(Ok(config())));
    let registry = HostInstanceRegistry::new(
        &info(2, true)?,
        provisioner.clone(),
        f.records.clone(),
        f.cipher.clone(),
    );
    let mut opening = Box::pin(registry.open_context(created.id));
    tokio::select! {
        result = &mut opening => return Err(format!("migration did not wait: {result:?}").into()),
        () = provisioner.entered.notified() => {}
    }
    let mut fields = config().into_object();
    fields.insert("label".into(), "competing-update".into());
    f.service()
        .update_instance(&reference, fields.into())
        .await?;
    provisioner.release.notify_one();
    let error = opening.await.err().ok_or("revision conflict")?;
    assert_eq!(
        error.storage_error().ok_or("category")?.kind(),
        StorageErrorKind::Conflict
    );
    let retained = f.records.get(reference.clone()).await?.ok_or("record")?;
    assert_eq!(retained.config_version(), 1);
    assert_eq!(retained.revision(), 2);
    assert_eq!(
        f.service()
            .get_instance(&reference)
            .await?
            .ok_or("instance")?
            .details["label"],
        "competing-update"
    );
    Ok(())
}

#[cfg(feature = "testing")]
impl ah_vfs::testing::RegistryFixture for Fixture {
    fn registry(&self) -> Arc<ah_vfs::application::host_instance_registry::HostInstanceRegistry> {
        Arc::new(HostInstanceRegistry::new(
            &self.driver.info,
            self.driver.provisioner.clone(),
            self.records.clone(),
            self.cipher.clone(),
        ))
    }
    fn config(&self) -> InstanceConfig {
        config()
    }
    fn disable_storage(&self) -> DriverFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
    fn corrupt(&self, id: InstanceId) -> DriverFuture<'_, ()> {
        Box::pin(async move {
            sqlx::query("UPDATE vfs_instance_records SET config_json = 'invalid-json' WHERE instance_id = ?").bind(id.as_str()).execute(&self.pool).await.map_err(ah_vfs::infrastructure::sqlite_error::database_error)?;
            Ok(())
        })
    }
    fn close_database(&self) -> DriverFuture<'_, ()> {
        Box::pin(async move {
            self.pool.close().await;
            Ok(())
        })
    }
}
#[cfg(feature = "testing")]
#[tokio::test]
async fn host_registry_passes_cursor_corruption_and_failure_contract() -> TestResult {
    let fixture = Fixture::new(true, true).await?;
    ah_vfs::testing::assert_registry_contract(&fixture).await?;
    assert_eq!(fixture.driver.opens.load(Ordering::SeqCst), 0);
    assert_eq!(
        fixture.driver.provisioner.prepares.load(Ordering::SeqCst),
        3
    );
    Ok(())
}

#[tokio::test]
async fn secret_ciphertext_cannot_be_reused_for_another_instance() -> TestResult {
    let f = Fixture::new(true, true).await?;
    let service = f.service();
    let one = service
        .create_instance(f.driver.info.key(), config())
        .await?;
    let two = service
        .create_instance(f.driver.info.key(), config())
        .await?;
    let source = f.records.get(f.reference(one.id)).await?.ok_or("source")?;
    let target = f
        .records
        .get(f.reference(two.id.clone()))
        .await?
        .ok_or("target")?;
    let original: serde_json::Value = serde_json::from_str(source.config_json())?;
    let mut corrupted: serde_json::Value = serde_json::from_str(target.config_json())?;
    corrupted["parameters"]["password"] = original["parameters"]["password"].clone();
    sqlx::query("UPDATE vfs_instance_records SET config_json = ? WHERE instance_id = ?")
        .bind(serde_json::to_string(&corrupted)?)
        .bind(two.id.as_str())
        .execute(&f.pool)
        .await?;
    let registry = HostInstanceRegistry::new(
        &f.driver.info,
        f.driver.provisioner.clone(),
        f.records.clone(),
        f.cipher.clone(),
    );
    assert!(registry.open_context(two.id).await.is_err());
    Ok(())
}

#[tokio::test]
async fn ambiguous_secret_annotations_are_rejected_before_persistence() -> TestResult {
    let f = Fixture::new(true, true).await?;
    let mut schema = schema(true);
    schema.insert(
        "allOf".into(),
        serde_json::json!([{"properties":{"hidden":{"x-secret":true}}}]),
    );
    let info = DriverInfo::builder()
        .key(f.driver.info.key().clone())
        .name("Ambiguous schema".into())
        .version(semver::Version::new(1, 0, 0))
        .child_mount_policy(ChildMountPolicy::Allow)
        .capabilities(StorageCapabilities::default())
        .instance_config_schema(schema)
        .build();
    let registry = HostInstanceRegistry::new(
        &info,
        f.driver.provisioner.clone(),
        f.records.clone(),
        f.cipher.clone(),
    );
    let error = registry.create(config()).await.err().ok_or("rejection")?;
    assert!(
        matches!(error,DriverError::Storage(error) if error.kind()==StorageErrorKind::UnsupportedOperation)
    );
    assert_eq!(f.driver.provisioner.prepares.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn secret_bearing_value_objects_and_errors_have_redacted_debug() -> TestResult {
    let raw = config();
    let id = InstanceId::parse("test-instance")?;
    let context = OpenContext::new(id, raw.clone());
    let prepared = PreparedConfig::new(
        raw,
        BTreeMap::from([("private".into(), "password-value-private".into())]),
    );
    let error = StorageError::with_source(
        StorageErrorKind::Backend,
        std::io::Error::other("password-value-private"),
    );
    for debug in [
        format!("{context:?}"),
        format!("{prepared:?}"),
        format!("{error:?}"),
    ] {
        assert!(!debug.contains("password-value-private"));
    }
    assert!(std::error::Error::source(&error).is_some());
    Ok(())
}

#[tokio::test]
async fn validation_diagnostics_do_not_echo_secret_values_as_field_names() -> TestResult {
    let f = Fixture::new(true, true).await?;
    let mut fields = config().into_object();
    fields.insert("password-value-private".into(), true.into());
    let error = f
        .service()
        .create_instance(f.driver.info.key(), fields.into())
        .await
        .err()
        .ok_or("validation")?;
    assert!(!format!("{error:?}").contains("password-value-private"));
    assert!(!error.to_string().contains("password-value-private"));
    Ok(())
}

#[test]
fn immutable_driver_schema_is_borrowed_without_reconstruction() -> TestResult {
    let info = info(1, true)?;
    let first = info.instance_config_schema().ok_or("schema")?;
    let second = info.instance_config_schema().ok_or("schema")?;
    assert!(std::ptr::eq(first, second));
    Ok(())
}

#[tokio::test]
async fn host_records_reopen_from_disk_without_plaintext_secrets() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("host.sqlite");
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(true);
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options.clone())
        .await?;
    let records = Arc::new(SqliteInstanceRecordRepository::from_pool(pool.clone()).await?);
    let cipher: Arc<dyn SecretCipher> = Arc::new(TestCipher::new());
    let provisioner = Arc::new(Provisioner::default());
    let info = info(1, true)?;
    let registry =
        HostInstanceRegistry::new(&info, provisioner.clone(), records, Some(cipher.clone()));
    let created = registry.create(config()).await?;
    pool.close().await;
    let bytes = std::fs::read(&path)?;
    for secret in [
        "password-value-private",
        "nested-token-private",
        "array-token-private",
    ] {
        assert!(
            !bytes
                .windows(secret.len())
                .any(|part| part == secret.as_bytes())
        );
    }
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    let records = Arc::new(SqliteInstanceRecordRepository::from_pool(pool).await?);
    let reopened = HostInstanceRegistry::new(&info, provisioner, records, Some(cipher));
    assert_eq!(
        reopened.get(created.id.clone()).await?,
        Some(created.clone())
    );
    let context = reopened.open_context(created.id).await?;
    assert!(context.config().get("password") == Some(&"password-value-private".into()));
    Ok(())
}

#[tokio::test]
async fn record_updates_reject_stale_revisions_without_overwriting() -> TestResult {
    let f = Fixture::new(true, true).await?;
    let created = f
        .service()
        .create_instance(f.driver.info.key(), config())
        .await?;
    let reference = f.reference(created.id);
    let old = f.records.get(reference.clone()).await?.ok_or("record")?;
    let next = InstanceRecord::new(
        reference.clone(),
        old.config_version(),
        2,
        old.config_json().into(),
        old.created_at(),
        old.updated_at() + 1,
    )?;
    f.records.update(next.clone(), 1).await?;
    let error = f.records.update(next, 1).await.err().ok_or("conflict")?;
    assert!(
        matches!(error,DriverError::Storage(error) if error.kind()==StorageErrorKind::Conflict)
    );
    assert_eq!(
        f.records.get(reference).await?.ok_or("record")?.revision(),
        2
    );
    Ok(())
}

struct SnapshotRepository {
    inner: Arc<SqliteInstanceRecordRepository>,
    reads: AtomicUsize,
    competing_update: Mutex<Option<(HostInstanceRegistry, InstanceConfig)>>,
}

impl InstanceRecordRepository for SnapshotRepository {
    fn get(&self, instance: DriverInstanceRef) -> DriverFuture<'_, Option<InstanceRecord>> {
        Box::pin(async move {
            self.reads.fetch_add(1, Ordering::SeqCst);
            let record = self.inner.get(instance.clone()).await?;
            let competing = self.competing_update.lock().map_err(|_| invalid())?.take();
            if let Some((registry, config)) = competing {
                // Commit a competing write after taking the snapshot but before
                // returning it, so the race is deterministic without sleeps.
                registry
                    .update(instance.instance_id().clone(), config)
                    .await?;
            }
            Ok(record)
        })
    }

    fn list(
        &self,
        driver: DriverKey,
        query: InstanceQuery,
    ) -> DriverFuture<'_, InstanceRecordPage> {
        self.inner.list(driver, query)
    }

    fn insert(&self, record: InstanceRecord) -> DriverFuture<'_, ()> {
        self.inner.insert(record)
    }

    fn update(&self, record: InstanceRecord, revision: u64) -> DriverFuture<'_, ()> {
        self.inner.update(record, revision)
    }

    fn remove(&self, instance: DriverInstanceRef) -> DriverFuture<'_, RemoveInstanceOutcome> {
        self.inner.remove(instance)
    }
}

#[tokio::test]
async fn registry_updates_reject_writes_that_interleave_with_the_config_snapshot() -> TestResult {
    let f = Fixture::new(true, true).await?;
    let created = f
        .service()
        .create_instance(f.driver.info.key(), config())
        .await?;
    let competing = HostInstanceRegistry::new(
        &f.driver.info,
        f.driver.provisioner.clone(),
        f.records.clone(),
        f.cipher.clone(),
    );
    let mut fields = config().into_object();
    fields.insert("label".into(), "competing-update".into());
    let repository = Arc::new(SnapshotRepository {
        inner: f.records.clone(),
        reads: AtomicUsize::new(0),
        competing_update: Mutex::new(Some((competing, fields.into()))),
    });
    let registry = HostInstanceRegistry::new(
        &f.driver.info,
        f.driver.provisioner.clone(),
        repository.clone(),
        f.cipher.clone(),
    );
    let error = registry
        .update(created.id.clone(), config())
        .await
        .err()
        .ok_or("conflict")?;
    assert_eq!(
        error.storage_error().ok_or("storage error")?.kind(),
        StorageErrorKind::Conflict
    );
    assert_eq!(repository.reads.load(Ordering::SeqCst), 1);
    let reference = f.reference(created.id);
    assert_eq!(
        f.records
            .get(reference.clone())
            .await?
            .ok_or("record")?
            .revision(),
        2
    );
    assert_eq!(
        f.service()
            .get_instance(&reference)
            .await?
            .ok_or("instance")?
            .details["label"],
        "competing-update"
    );
    Ok(())
}

#[tokio::test]
async fn updates_use_the_migrated_revision_without_reloading_the_record() -> TestResult {
    let f = Fixture::new(true, true).await?;
    let created = f
        .service()
        .create_instance(f.driver.info.key(), config())
        .await?;
    let repository = Arc::new(SnapshotRepository {
        inner: f.records.clone(),
        reads: AtomicUsize::new(0),
        competing_update: Mutex::new(None),
    });
    let provisioner = Arc::new(Provisioner::default());
    let registry = HostInstanceRegistry::new(
        &info(2, true)?,
        provisioner.clone(),
        repository.clone(),
        f.cipher.clone(),
    );
    let mut fields = config().into_object();
    fields.insert("label".into(), "after-migration".into());
    let updated = registry.update(created.id.clone(), fields.into()).await?;
    assert_eq!(updated.details["label"], "after-migration");
    assert_eq!(repository.reads.load(Ordering::SeqCst), 1);
    assert_eq!(provisioner.migrations.load(Ordering::SeqCst), 1);
    let record = f
        .records
        .get(f.reference(created.id))
        .await?
        .ok_or("record")?;
    assert_eq!(record.config_version(), 2);
    assert_eq!(record.revision(), 3);
    Ok(())
}

struct DefaultProvisioner;
impl InstanceProvisioner for DefaultProvisioner {
    fn prepare(&self, config: InstanceConfig) -> DriverFuture<'_, PreparedConfig> {
        Box::pin(async move { Ok(PreparedConfig::new(config, BTreeMap::new())) })
    }
}
#[tokio::test]
async fn optional_provisioner_defaults_are_explicit_and_side_effect_free() -> TestResult {
    let provisioner = DefaultProvisioner;
    let config = config();
    assert_eq!(provisioner.migrate_config(1, config.clone()).await?, config);
    assert!(
        matches!(provisioner.test_connection(&config).await,Err(DriverError::Storage(error)) if error.kind()==StorageErrorKind::UnsupportedOperation)
    );
    Ok(())
}

#[tokio::test]
async fn corrupt_lookahead_envelopes_fail_the_entire_page() -> TestResult {
    let f = Fixture::new(true, true).await?;
    let service = f.service();
    let one = service
        .create_instance(f.driver.info.key(), config())
        .await?;
    let two = service
        .create_instance(f.driver.info.key(), config())
        .await?;
    let last = if one.id.as_str() > two.id.as_str() {
        one.id
    } else {
        two.id
    };
    for malformed in [
        serde_json::json!({}),
        serde_json::json!({"parameters":{},"secret_paths":[],"details":[]}),
        serde_json::json!({"parameters":{},"secret_paths":["/missing"],"details":{}}),
        serde_json::json!({"parameters":{"secret":[256]},"secret_paths":["/secret"],"details":{}}),
        serde_json::json!({"parameters":{"secret":[1]},"secret_paths":["/secret","/secret"],"details":{}}),
        serde_json::json!({"parameters":[1],"secret_paths":["","/0"],"details":{}}),
    ] {
        sqlx::query("UPDATE vfs_instance_records SET config_json = ? WHERE instance_id = ?")
            .bind(malformed.to_string())
            .bind(last.as_str())
            .execute(&f.pool)
            .await?;
        let error = service
            .list_instances(f.driver.info.key(), InstanceQuery::new(NonZeroUsize::MIN)?)
            .await
            .err()
            .ok_or("corrupt lookahead must fail the entire page")?;
        assert_eq!(
            error.storage_error().ok_or("storage error")?.kind(),
            StorageErrorKind::InvalidData
        );
    }
    Ok(())
}

#[tokio::test]
async fn a_whole_config_secret_round_trips_through_the_persisted_envelope() -> TestResult {
    let f = Fixture::new(true, true).await?;
    let mut root_schema = schema(true);
    root_schema.insert("x-secret".into(), true.into());
    let declaration = DriverInfo::builder()
        .key(f.driver.info.key().clone())
        .name("Whole-config secret".into())
        .version(semver::Version::new(1, 0, 0))
        .child_mount_policy(ChildMountPolicy::Allow)
        .capabilities(StorageCapabilities::default())
        .instance_config_schema(root_schema)
        .build();
    let registry = HostInstanceRegistry::new(
        &declaration,
        f.driver.provisioner.clone(),
        f.records.clone(),
        f.cipher.clone(),
    );
    let created = registry.create(config()).await?;
    let record = f
        .records
        .get(f.reference(created.id.clone()))
        .await?
        .ok_or("record")?;
    let envelope: serde_json::Value = serde_json::from_str(record.config_json())?;
    assert_eq!(envelope["secret_paths"], serde_json::json!([""]));
    for secret in [
        "stable-endpoint",
        "password-value-private",
        "nested-token-private",
        "array-token-private",
    ] {
        assert!(!record.config_json().contains(secret));
    }
    assert_eq!(
        registry.get(created.id.clone()).await?,
        Some(created.clone())
    );
    let context = registry.open_context(created.id).await?;
    let restored = context.config();
    assert_eq!(restored["label"], "first");
    assert_eq!(restored["password"], "password-value-private");
    assert_eq!(restored["auth"], config()["auth"]);
    assert_eq!(restored["tokens"], config()["tokens"]);
    Ok(())
}
