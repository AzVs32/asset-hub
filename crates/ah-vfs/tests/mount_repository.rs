use ah_vfs::application::mount_runtime::MountRuntime;
mod common;

use ah_vfs::application::driver_catalog::DriverCatalog;
use ah_vfs::application::driver_info::DriverInfo;
use ah_vfs::application::dto::MountFilter;
use ah_vfs::application::error::{MountError, MountRepositoryError};
use ah_vfs::application::port::{DriverFuture, MountRepository, MountRepositoryFuture, Storage};
use ah_vfs::application::service::MountService;
use ah_vfs::domain::{
    ChildMountPolicy, DriverError, DriverInstance, DriverKey, InstanceId, Mount, MountId,
    MountTableError, VirtualPath,
};
use std::future::Future;
use std::io;
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll, Waker};

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct State {
    mounts: Vec<Mount>,
    inserts: usize,
    fail_next_insert: bool,
}

struct TestRepository(Mutex<State>);

impl TestRepository {
    fn new(mounts: Vec<Mount>) -> Self {
        Self(Mutex::new(State {
            mounts,
            inserts: 0,
            fail_next_insert: false,
        }))
    }

    fn lock(&self) -> Result<MutexGuard<'_, State>, MountRepositoryError> {
        self.0.lock().map_err(|_| {
            MountRepositoryError::Backend(Box::new(io::Error::other(
                "test repository lock poisoned",
            )))
        })
    }
}

impl MountRepository for TestRepository {
    fn get(&self, id: MountId) -> MountRepositoryFuture<'_, Option<Mount>> {
        Box::pin(async move {
            Ok(self
                .lock()?
                .mounts
                .iter()
                .find(|mount| mount.id() == id)
                .cloned())
        })
    }

    fn list(&self, filter: MountFilter) -> MountRepositoryFuture<'_, Vec<Mount>> {
        Box::pin(async move {
            let mounts: Vec<_> = self
                .lock()?
                .mounts
                .iter()
                .filter(|mount| {
                    filter
                        .get_enabled()
                        .is_none_or(|enabled| mount.enabled() == enabled)
                        && filter
                            .get_driver_key()
                            .is_none_or(|key| mount.driver_instance().driver_key() == key)
                })
                .cloned()
                .collect();
            Ok(mounts)
        })
    }

    fn insert(&self, mount: Mount) -> MountRepositoryFuture<'_, ()> {
        Box::pin(async move {
            let mut state = self.lock()?;
            if std::mem::take(&mut state.fail_next_insert) {
                return Err(MountRepositoryError::Backend(Box::new(io::Error::other(
                    "simulated write failure",
                ))));
            }
            if state
                .mounts
                .iter()
                .any(|existing| existing.id() == mount.id())
            {
                return Err(MountRepositoryError::DuplicateId(mount.id()));
            }
            if state
                .mounts
                .iter()
                .any(|existing| existing.virtual_path() == mount.virtual_path())
            {
                return Err(MountRepositoryError::PathOccupied(
                    mount.virtual_path().clone(),
                ));
            }
            state.mounts.push(mount);
            state.inserts += 1;
            Ok(())
        })
    }

    fn remove(&self, id: MountId) -> MountRepositoryFuture<'_, ()> {
        Box::pin(async move {
            let mut state = self.lock()?;
            let index = state
                .mounts
                .iter()
                .position(|mount| mount.id() == id)
                .ok_or(MountRepositoryError::NotFound(id))?;
            state.mounts.remove(index);
            Ok(())
        })
    }

    fn set_enabled(&self, id: MountId, enabled: bool) -> MountRepositoryFuture<'_, ()> {
        Box::pin(async move {
            let mut state = self.lock()?;
            let mount = state
                .mounts
                .iter_mut()
                .find(|mount| mount.id() == id)
                .ok_or(MountRepositoryError::NotFound(id))?;
            mount.set_enabled(enabled);
            Ok(())
        })
    }
}

struct UnopenedDriver {
    info: DriverInfo,
}

impl common::Backend for UnopenedDriver {
    fn info(&self) -> &DriverInfo {
        &self.info
    }

    fn open(
        &self,
        context: ah_vfs::application::port::OpenContext,
    ) -> DriverFuture<'_, Arc<dyn Storage>> {
        let instance_id = context.instance_id().clone();
        Box::pin(async move { Err(DriverError::InstanceNotFound(instance_id)) })
    }
}

fn catalog() -> Result<Arc<DriverCatalog>, Box<dyn std::error::Error>> {
    let mut catalog = DriverCatalog::new();
    for (key, policy) in [
        ("builtin.local", ChildMountPolicy::Allow),
        ("acme.s3", ChildMountPolicy::Deny),
    ] {
        common::register(
            &mut catalog,
            common::managed(Arc::new(UnopenedDriver {
                info: DriverInfo::builder()
                    .capabilities(ah_vfs::domain::StorageCapabilities::default())
                    .key(DriverKey::parse(key)?)
                    .name(key.into())
                    .version(semver::Version::new(0, 1, 0))
                    .child_mount_policy(policy)
                    .build(),
            })),
        )?;
    }
    Ok(Arc::new(catalog))
}

fn reference(driver: &str, id: &str) -> Result<DriverInstance, Box<dyn std::error::Error>> {
    Ok(DriverInstance::new(
        DriverKey::parse(driver)?,
        InstanceId::parse(id)?,
    ))
}

fn mount(
    path: &str,
    driver: &str,
    id: &str,
    enabled: bool,
) -> Result<Mount, Box<dyn std::error::Error>> {
    Ok(Mount::builder()
        .id(MountId::new())
        .virtual_path(VirtualPath::parse(path)?)
        .driver_instance(reference(driver, id)?)
        .enabled(enabled)
        .build())
}

// These test adapters complete immediately; no runtime is needed to drive them.
fn ready<T>(future: impl Future<Output = T>) -> Result<T, Box<dyn std::error::Error>> {
    let mut future = Box::pin(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(value) => Ok(value),
        Poll::Pending => Err("test adapter unexpectedly yielded".into()),
    }
}

#[test]
fn initialization_creates_one_root_and_is_idempotent() -> TestResult {
    let repository = Arc::new(TestRepository::new(Vec::new()));
    let service = MountService::new(Arc::new({
        let drivers = catalog()?;
        let records = common::Records::seeded(&drivers, &[])?;
        MountRuntime::new(repository.clone(), drivers, records, None)
    }));
    let instance = reference("builtin.local", "42")?;
    let id = ready(service.initialize_root(instance.clone()))??;
    let root = ready(repository.get(id))??.ok_or("root missing")?;
    assert!(root.virtual_path().is_root());
    assert!(root.enabled());
    assert_eq!(root.driver_instance(), &instance);
    assert_eq!(repository.lock()?.inserts, 1);

    assert_eq!(
        ready(service.initialize_root(reference("acme.s3", "99")?))??,
        id
    );
    let state = repository.lock()?;
    assert_eq!(state.inserts, 1);
    assert_eq!(state.mounts.len(), 1);
    assert_eq!(state.mounts[0].driver_instance(), &instance);
    Ok(())
}

#[test]
fn existing_unordered_namespace_is_restored_without_writing_or_reenabling() -> TestResult {
    let root = mount("/", "builtin.local", "42", false)?;
    let root_id = root.id();
    let child = mount("/data", "acme.s3", "99", true)?;
    let child_id = child.id();
    let repository = Arc::new(TestRepository::new(vec![child, root]));
    let service = MountService::new(Arc::new({
        let drivers = catalog()?;
        let records = common::Records::seeded(&drivers, &[])?;
        MountRuntime::new(repository.clone(), drivers, records, None)
    }));
    assert_eq!(
        ready(service.initialize_root(reference("unavailable.driver", "1")?))??,
        root_id
    );
    assert_eq!(repository.lock()?.inserts, 0);
    assert_eq!(
        ready(repository.get(root_id))??.map(|mount| mount.enabled()),
        Some(false)
    );
    assert_eq!(
        ready(repository.get(child_id))??.map(|mount| mount.enabled()),
        Some(true)
    );
    Ok(())
}

#[test]
fn invalid_existing_data_is_not_replaced_with_a_new_root() -> TestResult {
    let orphan = mount("/data", "builtin.local", "42", true)?;
    let orphan_id = orphan.id();
    let repository = Arc::new(TestRepository::new(vec![orphan]));
    let service = MountService::new(Arc::new({
        let drivers = catalog()?;
        let records = common::Records::seeded(&drivers, &[])?;
        MountRuntime::new(repository.clone(), drivers, records, None)
    }));
    assert!(matches!(
        ready(service.initialize_root(reference("builtin.local", "1")?))?,
        Err(MountError::Table(MountTableError::RootRequired))
    ));
    assert_eq!(repository.lock()?.inserts, 0);
    assert_eq!(repository.lock()?.mounts[0].id(), orphan_id);
    Ok(())
}

#[test]
fn missing_parent_driver_policy_does_not_drop_persisted_mounts() -> TestResult {
    let repository = Arc::new(TestRepository::new(vec![
        mount("/", "unavailable.driver", "42", true)?,
        mount("/data", "acme.s3", "99", true)?,
    ]));
    let service = MountService::new(Arc::new({
        let drivers = catalog()?;
        let records = common::Records::seeded(&drivers, &[])?;
        MountRuntime::new(repository.clone(), drivers, records, None)
    }));
    assert!(
        matches!(ready(service.initialize_root(reference("builtin.local", "1")?))?, Err(MountError::Table(MountTableError::PolicyUnavailable(key))) if key.as_str() == "unavailable.driver")
    );
    assert_eq!(repository.lock()?.inserts, 0);
    assert_eq!(repository.lock()?.mounts.len(), 2);
    Ok(())
}

#[test]
fn new_root_requires_a_registered_driver() -> TestResult {
    let repository = Arc::new(TestRepository::new(Vec::new()));
    let service = MountService::new(Arc::new({
        let drivers = catalog()?;
        let records = common::Records::seeded(&drivers, &[])?;
        MountRuntime::new(repository.clone(), drivers, records, None)
    }));
    assert!(
        matches!(ready(service.initialize_root(reference("unavailable.driver", "42")?))?, Err(MountError::DriverNotRegistered(key)) if key.as_str() == "unavailable.driver")
    );
    assert_eq!(repository.lock()?.inserts, 0);
    assert!(repository.lock()?.mounts.is_empty());
    Ok(())
}

#[test]
fn failed_initialization_keeps_namespace_empty_and_can_be_retried() -> TestResult {
    let repository = Arc::new(TestRepository::new(Vec::new()));
    repository.lock()?.fail_next_insert = true;
    let service = MountService::new(Arc::new({
        let drivers = catalog()?;
        let records = common::Records::seeded(&drivers, &[])?;
        MountRuntime::new(repository.clone(), drivers, records, None)
    }));
    let instance = reference("builtin.local", "42")?;
    assert!(matches!(
        ready(service.initialize_root(instance.clone()))?,
        Err(MountError::Repository(MountRepositoryError::Backend(_)))
    ));
    assert!(repository.lock()?.mounts.is_empty());
    let id = ready(service.initialize_root(instance))??;
    assert_eq!(repository.lock()?.mounts[0].id(), id);
    assert_eq!(repository.lock()?.inserts, 1);
    Ok(())
}
