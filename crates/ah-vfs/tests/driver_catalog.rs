use common::Backend;
mod common;

use ah_vfs::{
    application::{
        driver_catalog::DriverCatalog,
        driver_info::DriverInfo,
        error::DriverCatalogError,
        port::{
            Driver, DriverFuture, Storage, StorageFileReader, StorageFileWriter, StorageFuture,
        },
    },
    domain::{
        DriverError, DriverInstance, DriverKey, DriverKeyError, InstanceId,
        namespace::{ChildMountPolicy, Mount, MountId, MountTable, MountTableError, VirtualPath},
        storage::{
            StorageCreateDirRequest, StorageDirPage, StorageDirQuery, StorageEntry, StorageError,
            StorageErrorKind, StorageMoveEntryRequest, StorageReadFileRequest,
            StorageRemoveEntryRequest, StorageStatEntryRequest, StorageWriteFileRequest,
        },
    },
};

use std::future::Future;
use std::num::NonZeroUsize;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::task::{Context, Poll, Waker};

type TestResult = Result<(), Box<dyn std::error::Error>>;

// Catalog tests use an independent fake, not an unfinished storage adapter.
struct TestStorage;

fn unsupported<T: Send + 'static>() -> StorageFuture<'static, T> {
    Box::pin(async { Err(StorageError::new(StorageErrorKind::UnsupportedOperation)) })
}

impl Storage for TestStorage {
    fn chunk_size(&self) -> NonZeroUsize {
        NonZeroUsize::MIN
    }
    fn stat_entry(&self, _: StorageStatEntryRequest) -> StorageFuture<'_, StorageEntry> {
        unsupported()
    }
    fn list_entries(&self, _: StorageDirQuery) -> StorageFuture<'_, StorageDirPage> {
        unsupported()
    }
    fn read_file(
        &self,
        _: StorageReadFileRequest,
    ) -> StorageFuture<'_, Box<dyn StorageFileReader>> {
        unsupported()
    }
    fn write_file(
        &self,
        _: StorageWriteFileRequest,
    ) -> StorageFuture<'_, Box<dyn StorageFileWriter>> {
        unsupported()
    }
    fn create_dir(&self, _: StorageCreateDirRequest) -> StorageFuture<'_, StorageEntry> {
        unsupported()
    }
    fn remove_entry(&self, _: StorageRemoveEntryRequest) -> StorageFuture<'_, ()> {
        unsupported()
    }
    fn move_entry(&self, _: StorageMoveEntryRequest) -> StorageFuture<'_, StorageEntry> {
        unsupported()
    }
}

struct TestDriver {
    info: DriverInfo,
    storage: Arc<dyn Storage>,
    opens: AtomicUsize,
    fail: bool,
}

impl common::Backend for TestDriver {
    fn info(&self) -> &DriverInfo {
        &self.info
    }

    fn open(
        &self,
        context: ah_vfs::application::port::OpenContext,
    ) -> DriverFuture<'_, Arc<dyn Storage>> {
        let instance_id = context.instance_id().clone();
        Box::pin(async move {
            self.opens.fetch_add(1, Ordering::Relaxed);
            if self.fail || instance_id.as_str() != "42" {
                Err(DriverError::InstanceNotFound(instance_id))
            } else {
                Ok(self.storage.clone())
            }
        })
    }
}

fn driver(
    key: &str,
    name: &str,
    policy: ChildMountPolicy,
    fail: bool,
) -> Result<Arc<TestDriver>, Box<dyn std::error::Error>> {
    Ok(Arc::new(TestDriver {
        info: DriverInfo::builder()
            .capabilities(ah_vfs::domain::storage::StorageCapabilities::default())
            .key(DriverKey::parse(key)?)
            .name(name.into())
            .version(semver::Version::new(0, 1, 0))
            .child_mount_policy(policy)
            .build(),
        storage: Arc::new(TestStorage),
        opens: AtomicUsize::new(0),
        fail,
    }))
}

fn ready<T>(future: impl Future<Output = T>) -> Result<T, Box<dyn std::error::Error>> {
    let mut future = Box::pin(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(value) => Ok(value),
        Poll::Pending => Err("test driver unexpectedly yielded".into()),
    }
}

#[test]
fn all_key_inputs_preserve_identity_and_reject_invalid_characters() -> Result<(), DriverKeyError> {
    for input in ["builtin.local", "acme.s3", "Acme.s3", "插件.memory"] {
        let key = DriverKey::parse(input)?;
        assert_eq!(key.as_str(), input);
        assert_eq!(input.parse::<DriverKey>()?, key);
        assert_eq!(DriverKey::try_from(input.to_owned())?, key);
    }
    assert_ne!(DriverKey::parse("acme.s3")?, DriverKey::parse("Acme.s3")?);
    for input in [
        "",
        " ",
        " acme.s3",
        "acme.s3 ",
        "acme\ts3",
        "acme\ns3",
        "acme\0s3",
        "acme\u{2003}s3",
    ] {
        let expected = DriverKey::parse(input);
        assert!(expected.is_err(), "accepted {input:?}");
        assert_eq!(input.parse::<DriverKey>(), expected);
        assert_eq!(DriverKey::try_from(input.to_owned()), expected);
    }
    Ok(())
}

#[test]
fn duplicate_registration_preserves_the_original_driver_and_its_declarations() -> TestResult {
    let mut catalog = DriverCatalog::new();
    let original: Arc<dyn Driver> = common::managed(driver(
        "builtin.local",
        "Local",
        ChildMountPolicy::Allow,
        false,
    )?);
    let key = original.info().key().clone();
    assert!(catalog.get(&key).is_none());
    assert!(catalog.list().is_empty());
    common::register(&mut catalog, common::managed(original.clone()))?;
    let error = common::register(
        &mut catalog,
        common::managed(driver(
            "builtin.local",
            "Replacement",
            ChildMountPolicy::Deny,
            true,
        )?),
    );
    assert!(
        matches!(error, Err(DriverCatalogError::AlreadyRegistered(ref duplicate)) if duplicate == &key)
    );
    let retained = catalog.get(&key).ok_or("driver missing")?;
    assert!(Arc::ptr_eq(retained, &original));
    assert_eq!(retained.info().name(), "Local");
    assert_eq!(
        retained.info().child_mount_policy(),
        ChildMountPolicy::Allow
    );
    assert_eq!(catalog.list().len(), 1);
    Ok(())
}

#[test]
fn listing_and_removal_follow_registration_state() -> TestResult {
    let mut catalog = DriverCatalog::new();
    for key in ["acme.s3", "builtin.local", "acme.memory"] {
        common::register(
            &mut catalog,
            common::managed(driver(key, key, ChildMountPolicy::Allow, false)?),
        )?;
    }
    assert_eq!(
        catalog
            .list()
            .iter()
            .map(|driver| driver.info().key().as_str())
            .collect::<Vec<_>>(),
        ["acme.memory", "acme.s3", "builtin.local"]
    );
    let key = DriverKey::parse("acme.s3")?;
    let original = catalog.get(&key).ok_or("driver missing")?.clone();
    let removed = catalog.unregister(&key)?;
    assert!(Arc::ptr_eq(&original, &removed));
    assert_eq!(removed.info().key(), &key);
    assert!(catalog.get(&key).is_none());
    assert_eq!(catalog.list().len(), 2);
    assert!(
        matches!(catalog.unregister(&key), Err(DriverCatalogError::NotRegistered(ref missing)) if missing == &key)
    );
    common::register(&mut catalog, common::managed(removed))?;
    assert!(Arc::ptr_eq(
        catalog.get(&key).ok_or("driver missing")?,
        &original
    ));
    assert_eq!(catalog.list().len(), 3);
    Ok(())
}

#[test]
fn catalog_returns_the_original_driver_and_handles_outlive_registration() -> TestResult {
    let mut catalog = DriverCatalog::new();
    let original = driver("builtin.local", "Local", ChildMountPolicy::Allow, false)?;
    let key = original.info().key().clone();
    let expected = original.storage.clone();
    let erased: Arc<dyn Driver> = common::managed(original.clone());
    common::register(&mut catalog, common::managed(erased.clone()))?;
    let duplicate = driver("builtin.local", "Replacement", ChildMountPolicy::Deny, true)?;
    assert!(common::register(&mut catalog, common::managed(duplicate.clone())).is_err());
    let retained = catalog.get(&key).ok_or("driver missing")?.clone();
    assert!(Arc::ptr_eq(&retained, &erased));
    let storage = ready(retained.open(ah_vfs::application::port::OpenContext::new(
        InstanceId::parse("42")?,
        Default::default(),
    )))??;
    assert!(Arc::ptr_eq(&storage, &expected));
    assert_eq!(original.opens.load(Ordering::Relaxed), 1);
    assert_eq!(duplicate.opens.load(Ordering::Relaxed), 0);
    drop(catalog.unregister(&key)?);
    assert!(catalog.get(&key).is_none());
    let reopened = ready(retained.open(ah_vfs::application::port::OpenContext::new(
        InstanceId::parse("42")?,
        Default::default(),
    )))??;
    assert!(Arc::ptr_eq(&reopened, &expected));
    drop(retained);
    drop(erased);
    drop(original);
    assert_eq!(storage.chunk_size(), NonZeroUsize::MIN);
    Ok(())
}

#[test]
fn driver_opening_errors_reach_the_caller() -> TestResult {
    let mut catalog = DriverCatalog::new();
    let original = driver("acme.s3", "S3", ChildMountPolicy::Deny, true)?;
    let key = original.info().key().clone();
    common::register(&mut catalog, common::managed(original))?;
    assert!(matches!(
        ready(catalog.get(&key).ok_or("driver missing")?.open(
            ah_vfs::application::port::OpenContext::new(
                InstanceId::parse("42")?,
                Default::default()
            )
        ))?,
        Err(DriverError::InstanceNotFound(_))
    ));
    Ok(())
}

#[test]
fn mount_table_uses_driver_owned_declarations() -> TestResult {
    let mut catalog = DriverCatalog::new();
    for (key, policy) in [
        ("builtin.local", ChildMountPolicy::Allow),
        ("acme.s3", ChildMountPolicy::Deny),
    ] {
        common::register(
            &mut catalog,
            common::managed(driver(key, key, policy, false)?),
        )?;
    }
    let create_mount = |path: &str, key: &str| -> Result<Mount, Box<dyn std::error::Error>> {
        Ok(Mount::builder()
            .id(MountId::new())
            .virtual_path(VirtualPath::parse(path)?)
            .driver_instance(DriverInstance::new(
                DriverKey::parse(key)?,
                InstanceId::parse("42")?,
            ))
            .enabled(true)
            .build())
    };
    let policy = |key: &DriverKey| {
        catalog
            .get(key)
            .map(|driver| driver.info().child_mount_policy())
    };
    let mut table = MountTable::new(create_mount("/", "builtin.local")?)?;
    let archive = create_mount("/archive", "acme.s3")?;
    let archive_id = archive.id();
    table.insert(archive, policy)?;
    assert!(
        matches!(table.insert(create_mount("/archive/child", "builtin.local")?, policy),
        Err(MountTableError::ChildMountsForbidden { driver, .. }) if driver.as_str() == "acme.s3")
    );
    let path = VirtualPath::parse("/archive/report.txt")?;
    let resolved = table.resolve(&path)?;
    assert_eq!(resolved.mount().id(), archive_id);
    assert_eq!(resolved.relative_path(), "report.txt");
    let reference = resolved.mount().driver_instance();
    let selected = catalog
        .get(reference.driver_key())
        .ok_or("driver missing")?;
    ready(selected.open(ah_vfs::application::port::OpenContext::new(
        reference.instance_id().clone(),
        Default::default(),
    )))??;
    Ok(())
}
