#[path = "../../../tests/common/mod.rs"]
mod common;
use super::*;
use crate::{
    application::{
        driver_info::DriverInfo,
        dto::*,
        error::MountRepositoryError,
        port::{DriverFuture, MountRepositoryFuture, StorageFuture, StoragePermissions},
        service::{EntryService, InstanceService, MountService},
    },
    domain::{
        ChildMountPolicy, DriverKey, InstanceId, MountId, MountTableError, StorageCapabilities,
        StorageError, StorageErrorKind, VirtualPath,
    },
};
use std::{
    error::Error,
    future::{Future, pending, poll_fn},
    io,
    num::NonZeroUsize,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    task::Poll,
};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

#[derive(Default)]
struct Repository {
    mounts: Mutex<Vec<Mount>>,
    lists: AtomicUsize,
    fail_list: AtomicBool,
    suspend_write: AtomicBool,
    suspend_list: AtomicBool,
    list_gate: tokio::sync::Notify,
}

impl MountRepository for Repository {
    fn get(&self, id: MountId) -> MountRepositoryFuture<'_, Option<Mount>> {
        Box::pin(async move {
            Ok(self
                .mounts
                .lock()
                .map_err(lock_error)?
                .iter()
                .find(|m| m.id() == id)
                .cloned())
        })
    }
    fn list(&self, filter: MountFilter) -> MountRepositoryFuture<'_, Vec<Mount>> {
        Box::pin(async move {
            self.lists.fetch_add(1, Ordering::SeqCst);
            if self.suspend_list.load(Ordering::SeqCst) {
                self.list_gate.notified().await;
            }
            if self.fail_list.load(Ordering::SeqCst) {
                return Err(lock_error("simulated read failure"));
            }
            Ok(self
                .mounts
                .lock()
                .map_err(lock_error)?
                .iter()
                .filter(|m| {
                    filter
                        .get_driver_key()
                        .is_none_or(|key| m.driver_instance().driver_key() == key)
                        && filter
                            .get_enabled()
                            .is_none_or(|enabled| m.enabled() == enabled)
                })
                .cloned()
                .collect())
        })
    }
    fn insert(&self, mount: Mount) -> MountRepositoryFuture<'_, ()> {
        Box::pin(async move {
            self.mounts.lock().map_err(lock_error)?.push(mount);
            if self.suspend_write.load(Ordering::SeqCst) {
                pending::<()>().await;
            }
            Ok(())
        })
    }
    fn remove(&self, id: MountId) -> MountRepositoryFuture<'_, ()> {
        Box::pin(async move {
            self.mounts
                .lock()
                .map_err(lock_error)?
                .retain(|m| m.id() != id);
            Ok(())
        })
    }
    fn set_enabled(&self, id: MountId, enabled: bool) -> MountRepositoryFuture<'_, ()> {
        Box::pin(async move {
            let mut mounts = self.mounts.lock().map_err(lock_error)?;
            let mount = mounts
                .iter_mut()
                .find(|m| m.id() == id)
                .ok_or(MountRepositoryError::NotFound(id))?;
            mount.set_enabled(enabled);
            Ok(())
        })
    }
}

fn lock_error(_: impl std::fmt::Display) -> MountRepositoryError {
    MountRepositoryError::Backend(Box::new(io::Error::other("test persistence failed")))
}

struct TestStorage {
    validations: Arc<AtomicUsize>,
    capabilities: StorageCapabilities,
    generation: usize,
    current_generation: Arc<AtomicUsize>,
}

fn capabilities() -> StorageCapabilities {
    StorageCapabilities {
        stat_entry: true,
        ..Default::default()
    }
}

impl Storage for TestStorage {
    fn is_stale(&self) -> bool {
        self.generation != self.current_generation.load(Ordering::SeqCst)
    }
    fn capabilities(&self) -> StorageCapabilities {
        self.capabilities
    }
    fn permissions(&self) -> Option<&dyn StoragePermissions> {
        self.validations.fetch_add(1, Ordering::SeqCst);
        None
    }
    fn chunk_size(&self) -> NonZeroUsize {
        NonZeroUsize::MIN
    }
    fn stat_entry(&self, request: StorageStatEntryRequest) -> StorageFuture<'_, StorageEntry> {
        Box::pin(async move {
            Ok(StorageEntry {
                path: request.path,
                kind: StorageEntryKind::File,
                size: Some(1),
                modified: None,
            })
        })
    }
}

struct TestDriver {
    info: DriverInfo,
    opens: AtomicUsize,
    validations: Arc<AtomicUsize>,
    fail_open: AtomicBool,
    suspend_open: AtomicBool,
    open_gate: tokio::sync::Notify,
    invalid_capabilities: AtomicBool,
    suspend_remove: AtomicBool,
    uncertain_remove: AtomicBool,
    generation: Arc<AtomicUsize>,
}

impl TestDriver {
    fn new(key: &str) -> TestResult<Self> {
        Ok(Self {
            info: DriverInfo::builder()
                .key(DriverKey::parse(key)?)
                .name(key.into())
                .version(semver::Version::new(1, 0, 0))
                .child_mount_policy(ChildMountPolicy::Allow)
                .capabilities(capabilities())
                .build(),
            opens: AtomicUsize::new(0),
            validations: Arc::new(AtomicUsize::new(0)),
            fail_open: AtomicBool::new(false),
            suspend_open: AtomicBool::new(false),
            open_gate: tokio::sync::Notify::new(),
            invalid_capabilities: AtomicBool::new(false),
            suspend_remove: AtomicBool::new(false),
            uncertain_remove: AtomicBool::new(false),
            generation: Arc::new(AtomicUsize::new(1)),
        })
    }
    fn reference(&self, id: &str) -> TestResult<DriverInstanceRef> {
        Ok(DriverInstanceRef::new(
            self.info.key().clone(),
            InstanceId::parse(id)?,
        ))
    }
}

impl common::Backend for TestDriver {
    fn registry(self: Arc<Self>) -> Arc<dyn crate::application::port::InstanceRegistry> {
        self
    }

    fn info(&self) -> &DriverInfo {
        &self.info
    }
    fn open(
        &self,
        context: ah_vfs::application::port::OpenContext,
    ) -> DriverFuture<'_, Arc<dyn Storage>> {
        let _ = context.instance_id().clone();
        Box::pin(async move {
            self.opens.fetch_add(1, Ordering::SeqCst);
            if self.suspend_open.load(Ordering::SeqCst) {
                self.open_gate.notified().await;
            }
            if self.fail_open.load(Ordering::SeqCst) {
                return Err(StorageError::new(StorageErrorKind::Unavailable).into());
            }
            let storage: Arc<dyn Storage> = Arc::new(TestStorage {
                generation: self.generation.load(Ordering::SeqCst),
                current_generation: self.generation.clone(),
                validations: self.validations.clone(),
                capabilities: if self.invalid_capabilities.load(Ordering::SeqCst) {
                    StorageCapabilities {
                        read_file: crate::domain::FileReadSupport::Range,
                        ..capabilities()
                    }
                } else {
                    capabilities()
                },
            });
            Ok(storage)
        })
    }
}
impl ah_vfs::application::port::InstanceRegistry for TestDriver {
    fn create(&self, _: InstanceConfig) -> DriverFuture<'_, InstanceInfo> {
        Box::pin(async { Err(StorageError::new(StorageErrorKind::UnsupportedOperation).into()) })
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
        Box::pin(async move {
            if self.suspend_remove.load(Ordering::SeqCst) {
                pending::<()>().await;
            }
            if self.uncertain_remove.load(Ordering::SeqCst) {
                return Err(StorageError::new(StorageErrorKind::OutcomeUnknown).into());
            }
            Ok(RemoveInstanceOutcome::Removed)
        })
    }
}

struct Fixture {
    runtime: Arc<MountRuntime>,
    repository: Arc<Repository>,
    driver: Arc<TestDriver>,
    mounts: MountService,
    entries: EntryService,
    instances: InstanceService,
}

impl Fixture {
    fn new() -> TestResult<Self> {
        let repository = Arc::new(Repository::default());
        let driver = Arc::new(TestDriver::new("external.demo")?);
        let mut drivers = DriverCatalog::new();
        common::register(&mut drivers, common::managed(driver.clone()))?;
        let runtime = Arc::new(MountRuntime::new(repository.clone(), Arc::new(drivers)));
        Ok(Self {
            mounts: MountService::new(runtime.clone()),
            entries: EntryService::new(runtime.clone()),
            instances: InstanceService::new(runtime.clone()),
            runtime,
            repository,
            driver,
        })
    }
    async fn mount(&self, path: &str, id: &str) -> TestResult<MountView> {
        Ok(self
            .mounts
            .mount(MountRequest {
                virtual_path: VirtualPath::parse(path)?,
                driver_instance: self.driver.reference(id)?,
            })
            .await?)
    }
}

async fn poll_once<T>(future: std::pin::Pin<&mut impl Future<Output = T>>) -> Poll<T> {
    let mut future = future;
    poll_fn(|cx| Poll::Ready(future.as_mut().poll(cx))).await
}

#[tokio::test]
async fn warm_io_reuses_namespace_and_the_handle_opened_during_mount() -> TestResult {
    let f = Fixture::new()?;
    f.mount("/", "one").await?;
    let second_entry_service = EntryService::new(f.runtime.clone());
    f.entries.stat_entry(&VirtualPath::parse("/file")?).await?;
    let lists = f.repository.lists.load(Ordering::SeqCst);
    f.repository.fail_list.store(true, Ordering::SeqCst);
    f.driver.fail_open.store(true, Ordering::SeqCst);
    for _ in 0..20 {
        second_entry_service
            .stat_entry(&VirtualPath::parse("/file")?)
            .await?;
        f.entries.capabilities(&VirtualPath::root()).await?;
    }
    assert_eq!(f.repository.lists.load(Ordering::SeqCst), lists);
    assert_eq!(f.driver.opens.load(Ordering::SeqCst), 1);
    assert_eq!(f.driver.validations.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn stale_cached_handles_are_reopened_without_rebinding_retained_handles() -> TestResult {
    let f = Fixture::new()?;
    let reference = f.driver.reference("one")?;
    let driver = common::managed(f.driver.clone());
    let old = f.runtime.storage(&reference, driver.as_ref()).await?;
    f.driver.generation.fetch_add(1, Ordering::SeqCst);
    assert!(old.is_stale());
    f.driver.fail_open.store(true, Ordering::SeqCst);
    assert!(
        f.runtime
            .storage(&reference, driver.as_ref())
            .await
            .is_err()
    );
    f.driver.fail_open.store(false, Ordering::SeqCst);
    let new = f.runtime.storage(&reference, driver.as_ref()).await?;
    assert!(!new.is_stale());
    assert!(!Arc::ptr_eq(&old, &new));
    assert!(old.is_stale());
    let cached = f.runtime.storage(&reference, driver.as_ref()).await?;
    assert!(Arc::ptr_eq(&new, &cached));
    assert_eq!(f.driver.opens.load(Ordering::SeqCst), 3);
    Ok(())
}

#[tokio::test]
async fn namespace_mutations_invalidate_tables_and_preserve_shared_instance_handles() -> TestResult
{
    let f = Fixture::new()?;
    f.mounts.initialize_root(f.driver.reference("one")?).await?;
    f.entries.capabilities(&VirtualPath::root()).await?;
    let child = f.mount("/child", "one").await?;
    f.entries
        .capabilities(&VirtualPath::parse("/child")?)
        .await?;
    f.mounts.disable(child.id).await?;
    assert!(
        matches!(f.entries.capabilities(&VirtualPath::parse("/child")?).await, Err(EntryError::Table(MountTableError::Disabled(id))) if id == child.id)
    );
    f.mounts.enable(child.id).await?;
    f.entries
        .capabilities(&VirtualPath::parse("/child")?)
        .await?;
    f.mounts.unmount(child.id).await?;
    assert!(f.runtime.table().await?.get(child.id).is_none());
    assert_eq!(f.driver.opens.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn storage_identity_includes_both_driver_key_and_instance_id() -> TestResult {
    let one = Arc::new(TestDriver::new("one")?);
    let two = Arc::new(TestDriver::new("two")?);
    let mut catalog = DriverCatalog::new();
    common::register(&mut catalog, common::managed(one.clone()))?;
    common::register(&mut catalog, common::managed(two.clone()))?;
    let runtime = MountRuntime::new(Arc::new(Repository::default()), Arc::new(catalog));
    let a = runtime
        .storage(
            &one.reference("same")?,
            runtime
                .drivers
                .get(one.info.key())
                .ok_or("driver")?
                .as_ref(),
        )
        .await?;
    let a2 = runtime
        .storage(
            &one.reference("same")?,
            runtime
                .drivers
                .get(one.info.key())
                .ok_or("driver")?
                .as_ref(),
        )
        .await?;
    let b = runtime
        .storage(
            &two.reference("same")?,
            runtime
                .drivers
                .get(two.info.key())
                .ok_or("driver")?
                .as_ref(),
        )
        .await?;
    let c = runtime
        .storage(
            &one.reference("other")?,
            runtime
                .drivers
                .get(one.info.key())
                .ok_or("driver")?
                .as_ref(),
        )
        .await?;
    assert!(Arc::ptr_eq(&a, &a2));
    assert!(!Arc::ptr_eq(&a, &b));
    assert!(!Arc::ptr_eq(&a, &c));
    Ok(())
}

#[tokio::test]
async fn removal_and_uncertain_removal_evict_retention_without_revoking_existing_handles()
-> TestResult {
    for uncertain in [false, true] {
        let f = Fixture::new()?;
        f.mount("/", "root").await?;
        let reference = f.driver.reference("unmounted")?;
        let retained = f
            .runtime
            .storage(
                &reference,
                f.runtime
                    .drivers
                    .get(f.driver.info.key())
                    .ok_or("driver")?
                    .as_ref(),
            )
            .await?;
        let weak = Arc::downgrade(&retained);
        f.driver.uncertain_remove.store(uncertain, Ordering::SeqCst);
        let result = f.instances.remove_instance(&reference).await;
        if uncertain {
            assert_eq!(
                result
                    .err()
                    .ok_or("unknown")?
                    .storage_error()
                    .ok_or("domain")?
                    .kind(),
                StorageErrorKind::OutcomeUnknown
            );
        } else {
            assert_eq!(result?, RemoveInstanceOutcome::Removed);
        }
        assert!(weak.upgrade().is_some());
        drop(retained);
        assert!(weak.upgrade().is_none());
    }
    Ok(())
}

#[tokio::test]
async fn failures_are_retryable_and_explicit_invalidation_reloads_external_state() -> TestResult {
    let f = Fixture::new()?;
    assert!(f.runtime.table().await.is_err());
    f.mounts.initialize_root(f.driver.reference("one")?).await?;
    f.driver.fail_open.store(true, Ordering::SeqCst);
    assert!(f.entries.capabilities(&VirtualPath::root()).await.is_err());
    f.driver.fail_open.store(false, Ordering::SeqCst);
    f.entries.capabilities(&VirtualPath::root()).await?;
    assert_eq!(f.driver.opens.load(Ordering::SeqCst), 2);
    let old = f.runtime.table().await?;
    f.runtime.invalidate();
    let new = f.runtime.table().await?;
    assert!(!Arc::ptr_eq(&old, &new));
    f.entries.capabilities(&VirtualPath::root()).await?;
    assert_eq!(f.driver.opens.load(Ordering::SeqCst), 3);
    Ok(())
}

#[tokio::test]
async fn cancelled_namespace_writes_invalidate_snapshots_loaded_while_waiting() -> TestResult {
    let f = Fixture::new()?;
    f.mount("/", "root").await?;
    f.runtime.table().await?;
    f.repository.suspend_write.store(true, Ordering::SeqCst);
    let mut mount = Box::pin(f.mount("/child", "child"));
    assert!(poll_once(mount.as_mut()).await.is_pending());
    let during = f.runtime.table().await?;
    drop(mount);
    let after = f.runtime.table().await?;
    assert!(!Arc::ptr_eq(&during, &after));
    assert!(after.get_by_path(&VirtualPath::parse("/child")?).is_some());
    Ok(())
}

#[tokio::test]
async fn cold_opens_are_merged_per_identity_and_other_instances_do_not_wait() -> TestResult {
    let f = Fixture::new()?;
    let reference = f.driver.reference("one")?;
    f.driver.suspend_open.store(true, Ordering::SeqCst);
    let mut first = Box::pin(
        f.runtime.storage(
            &reference,
            f.runtime
                .drivers
                .get(f.driver.info.key())
                .ok_or("driver")?
                .as_ref(),
        ),
    );
    let mut second = Box::pin(
        f.runtime.storage(
            &reference,
            f.runtime
                .drivers
                .get(f.driver.info.key())
                .ok_or("driver")?
                .as_ref(),
        ),
    );
    assert!(poll_once(first.as_mut()).await.is_pending());
    assert!(poll_once(second.as_mut()).await.is_pending());
    assert_eq!(f.driver.opens.load(Ordering::SeqCst), 1);
    f.driver.suspend_open.store(false, Ordering::SeqCst);
    let other = f.driver.reference("other")?;
    f.runtime
        .storage(
            &other,
            f.runtime
                .drivers
                .get(f.driver.info.key())
                .ok_or("driver")?
                .as_ref(),
        )
        .await?;
    drop(first);
    second.await?;
    assert_eq!(f.driver.opens.load(Ordering::SeqCst), 3);
    Ok(())
}

#[tokio::test]
async fn invalidated_inflight_open_cannot_repopulate_the_current_cache() -> TestResult {
    let f = Fixture::new()?;
    let reference = f.driver.reference("one")?;
    f.driver.suspend_open.store(true, Ordering::SeqCst);
    let mut pending_open = Box::pin(
        f.runtime.storage(
            &reference,
            f.runtime
                .drivers
                .get(f.driver.info.key())
                .ok_or("driver")?
                .as_ref(),
        ),
    );
    assert!(poll_once(pending_open.as_mut()).await.is_pending());
    f.runtime.invalidate_instance(&reference);
    f.driver.suspend_open.store(false, Ordering::SeqCst);
    let current = f
        .runtime
        .storage(
            &reference,
            f.runtime
                .drivers
                .get(f.driver.info.key())
                .ok_or("driver")?
                .as_ref(),
        )
        .await?;
    f.driver.open_gate.notify_one();
    let outdated = pending_open.await?;
    assert!(!Arc::ptr_eq(&outdated, &current));
    let cached = f
        .runtime
        .storage(
            &reference,
            f.runtime
                .drivers
                .get(f.driver.info.key())
                .ok_or("driver")?
                .as_ref(),
        )
        .await?;
    assert!(Arc::ptr_eq(&current, &cached));
    assert_eq!(f.driver.opens.load(Ordering::SeqCst), 2);
    Ok(())
}

#[tokio::test]
async fn table_initialization_is_shared_and_detached_loads_cannot_replace_new_snapshots()
-> TestResult {
    let f = Fixture::new()?;
    f.mount("/", "root").await?;
    let baseline = f.repository.lists.load(Ordering::SeqCst);
    f.repository.suspend_list.store(true, Ordering::SeqCst);
    let mut old = Box::pin(f.runtime.table());
    let mut shared = Box::pin(f.runtime.table());
    assert!(poll_once(old.as_mut()).await.is_pending());
    assert!(poll_once(shared.as_mut()).await.is_pending());
    assert_eq!(f.repository.lists.load(Ordering::SeqCst), baseline + 1);
    f.runtime.invalidate();
    f.repository.suspend_list.store(false, Ordering::SeqCst);
    let current = f.runtime.table().await?;
    f.repository.list_gate.notify_one();
    let previous = old.await?;
    assert!(Arc::ptr_eq(&previous, &shared.await?));
    assert!(!Arc::ptr_eq(&previous, &current));
    assert!(Arc::ptr_eq(&current, &f.runtime.table().await?));
    assert_eq!(f.repository.lists.load(Ordering::SeqCst), baseline + 2);
    Ok(())
}

#[tokio::test]
async fn invalid_capabilities_are_not_retained_as_successful_handles() -> TestResult {
    let f = Fixture::new()?;
    let reference = f.driver.reference("one")?;
    f.driver.invalid_capabilities.store(true, Ordering::SeqCst);
    let error = f
        .runtime
        .storage(
            &reference,
            f.runtime
                .drivers
                .get(f.driver.info.key())
                .ok_or("driver")?
                .as_ref(),
        )
        .await
        .err()
        .ok_or("invalid adapter")?;
    assert!(
        matches!(error, DriverError::Storage(error) if error.kind() == StorageErrorKind::InvalidData)
    );
    f.driver.invalid_capabilities.store(false, Ordering::SeqCst);
    f.runtime
        .storage(
            &reference,
            f.runtime
                .drivers
                .get(f.driver.info.key())
                .ok_or("driver")?
                .as_ref(),
        )
        .await?;
    assert_eq!(f.driver.opens.load(Ordering::SeqCst), 2);
    Ok(())
}

#[tokio::test]
async fn cancelled_removal_also_evicts_handles_opened_while_waiting() -> TestResult {
    let f = Fixture::new()?;
    let reference = f.driver.reference("unmounted")?;
    let before = f
        .runtime
        .storage(
            &reference,
            f.runtime
                .drivers
                .get(f.driver.info.key())
                .ok_or("driver")?
                .as_ref(),
        )
        .await?;
    let before_weak = Arc::downgrade(&before);
    drop(before);
    f.driver.suspend_remove.store(true, Ordering::SeqCst);
    let mut remove = Box::pin(f.instances.remove_instance(&reference));
    assert!(poll_once(remove.as_mut()).await.is_pending());
    assert!(before_weak.upgrade().is_none());
    let during = f
        .runtime
        .storage(
            &reference,
            f.runtime
                .drivers
                .get(f.driver.info.key())
                .ok_or("driver")?
                .as_ref(),
        )
        .await?;
    let during_weak = Arc::downgrade(&during);
    drop(during);
    drop(remove);
    assert!(during_weak.upgrade().is_none());
    Ok(())
}
