use std::sync::Arc;

use asset_infra::driver::{LocalDriver, MemoryDriver};
use asset_infra::repository::SqliteMountRepository;
use asset_vfs::driver::{BoundDriver, Driver, DriverError, DriverKind, DriverPath};
use asset_vfs::error::VfsError;
use asset_vfs::mount::{Mount, MountError, MountId, MountRepository};
use asset_vfs::namespace::VirtualPath;
use asset_vfs::{DriverService, MountService};

struct MetadataOnlyDriver {
    kind: DriverKind,
    allows_submounts: bool,
}

impl MetadataOnlyDriver {
    fn new(kind: &str, allows_submounts: bool) -> Self {
        Self {
            kind: DriverKind::try_from(kind).unwrap(),
            allows_submounts,
        }
    }
}

impl Driver for MetadataOnlyDriver {
    fn kind(&self) -> DriverKind {
        self.kind.clone()
    }

    fn allows_submounts(&self) -> bool {
        self.allows_submounts
    }

    fn validate_path(&self, _root: &DriverPath) -> Result<(), VfsError> {
        panic!("mount queries must not validate backend paths")
    }

    fn bind(&self, _root: &DriverPath) -> Result<Box<dyn BoundDriver>, VfsError> {
        panic!("mount queries must not bind driver backends")
    }
}

#[tokio::test]
async fn mount_queries_use_registered_metadata_and_preserve_unknown_or_disabled_definitions() {
    let directory = tempfile::tempdir().unwrap();
    let repository = Arc::new(
        SqliteMountRepository::open(directory.path().join("mounts.sqlite"))
            .await
            .unwrap(),
    );
    let factories: Vec<Arc<dyn Driver>> = vec![
        Arc::new(MetadataOnlyDriver::new("custom.allow", true)),
        Arc::new(MetadataOnlyDriver::new("custom.deny", false)),
    ];
    let drivers = DriverService::new(factories).unwrap();
    let service = MountService::new(repository.clone(), Arc::new(drivers));
    let cases = [
        ("/", "custom.allow", true, Some(true)),
        ("/disabled", "custom.deny", false, Some(false)),
        ("/unknown", "unavailable", true, None),
    ];
    let mut expected = Vec::new();
    for (path, kind, enabled, policy) in cases {
        let mount = Mount::new(
            MountId::new(),
            VirtualPath::try_from(path).unwrap(),
            DriverKind::try_from(kind).unwrap(),
            DriverPath::new("unavailable-root"),
            enabled,
        );
        repository.insert(&mount).await.unwrap();
        expected.push((mount, policy));
    }

    let listing = service.list_mounts().await.unwrap();
    assert_eq!(listing.len(), expected.len());
    for (info, (mount, policy)) in listing.iter().zip(&expected) {
        assert_eq!(info.mount(), mount);
        assert_eq!(info.allows_submounts(), *policy);
        assert_eq!(
            service.mount_info(mount.id()).await.unwrap().as_ref(),
            Some(info)
        );
        assert_eq!(
            repository.get(mount.id()).await.unwrap().as_ref(),
            Some(mount)
        );
    }
    assert!(service.mount_info(MountId::new()).await.unwrap().is_none());
    let resolved = service
        .resolve_mount(&VirtualPath::try_from("/unknown/file").unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resolved.mount().driver().as_str(), "unavailable");
    assert_eq!(resolved.relative_path().as_str(), "file");
}

fn definition(path: &str, kind: &str, root: &str, enabled: bool) -> Mount {
    Mount::new(
        MountId::new(),
        VirtualPath::try_from(path).unwrap(),
        DriverKind::try_from(kind).unwrap(),
        DriverPath::new(root),
        enabled,
    )
}

async fn services(file: &std::path::Path) -> (Arc<SqliteMountRepository>, MountService) {
    let repository = Arc::new(SqliteMountRepository::open(file).await.unwrap());
    let factories: Vec<Arc<dyn Driver>> =
        vec![Arc::new(LocalDriver), Arc::new(MemoryDriver::new())];
    let service = MountService::new(
        repository.clone(),
        Arc::new(DriverService::new(factories).unwrap()),
    );
    (repository, service)
}

#[tokio::test]
async fn lifecycle_prefers_active_paths_retains_disabled_definitions_and_resolves_segments() {
    let directory = tempfile::tempdir().unwrap();
    let (repository, service) = services(&directory.path().join("mounts.sqlite")).await;
    assert!(
        service
            .resolve_mount(&VirtualPath::root())
            .await
            .unwrap()
            .is_none()
    );
    let root = definition("/", "local", directory.path().to_str().unwrap(), true);
    service.mount(root.clone()).await.unwrap();
    let first = definition("/cache", "memory", "/", false);
    let second = definition("/cache", "memory", "/", false);
    let active = definition("/cache", "memory", "/", true);
    for mount in [&first, &second, &active] {
        service.mount(mount.clone()).await.unwrap();
    }
    let cache = VirtualPath::try_from("/cache").unwrap();
    assert_eq!(
        service.mount_info(&cache).await.unwrap().unwrap().mount(),
        &active
    );
    assert_eq!(
        service
            .mount_info(first.id())
            .await
            .unwrap()
            .unwrap()
            .mount(),
        &first
    );
    let before = repository.list().await.unwrap();
    assert!(matches!(
        service
            .mount(definition("/cache", "memory", "/", true))
            .await,
        Err(VfsError::Mount(MountError::DuplicatePath(_)))
    ));
    assert_eq!(repository.list().await.unwrap(), before);

    let request = VirtualPath::try_from("/cache/file").unwrap();
    let resolved = service.resolve_mount(&request).await.unwrap().unwrap();
    assert_eq!(resolved.mount(), &active);
    assert_eq!(resolved.relative_path().as_str(), "file");
    let exact = service.resolve_mount(&cache).await.unwrap().unwrap();
    assert_eq!(exact.relative_path().as_str(), "");
    let adjacent = service
        .resolve_mount(&VirtualPath::try_from("/cachex/file").unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(adjacent.mount(), &root);
    assert_eq!(adjacent.relative_path().as_str(), "cachex/file");

    let disabled = service.unmount(&cache).await.unwrap().unwrap();
    assert!(!disabled.mount().enabled());
    assert_eq!(disabled.mount().id(), active.id());
    assert_eq!(repository.list().await.unwrap().len(), 4);
    assert!(matches!(
        service.mount_info(&cache).await,
        Err(VfsError::Mount(MountError::AmbiguousPath(_)))
    ));
    assert!(matches!(
        service.unmount(&cache).await,
        Err(VfsError::Mount(MountError::AmbiguousPath(_)))
    ));
    assert!(
        !service
            .unmount(first.id())
            .await
            .unwrap()
            .unwrap()
            .mount()
            .enabled()
    );
    assert_eq!(
        service
            .resolve_mount(&request)
            .await
            .unwrap()
            .unwrap()
            .mount(),
        &root
    );
    let mut enabled = disabled.mount().clone();
    enabled.enable();
    service.mount(enabled.clone()).await.unwrap();
    assert_eq!(
        service.mount_info(&cache).await.unwrap().unwrap().mount(),
        &enabled
    );
    assert_eq!(repository.list().await.unwrap().len(), 4);
    assert!(service.unmount(MountId::new()).await.unwrap().is_none());

    assert!(matches!(
        service.unmount(VirtualPath::root()).await,
        Err(VfsError::Mount(MountError::RootRequired))
    ));
    let mut disabled_root = root.clone();
    disabled_root.disable();
    assert!(matches!(
        service.mount(disabled_root).await,
        Err(VfsError::Mount(MountError::RootRequired))
    ));
    assert_eq!(repository.get(root.id()).await.unwrap(), Some(root));
}

#[tokio::test]
async fn submount_policies_and_backend_validation_apply_only_to_enabled_mounts() {
    let directory = tempfile::tempdir().unwrap();
    let (repository, service) = services(&directory.path().join("mounts.sqlite")).await;
    let local_root = directory.path().to_str().unwrap();
    service
        .mount(definition("/", "local", local_root, true))
        .await
        .unwrap();
    service
        .mount(definition("/mem", "memory", "/", true))
        .await
        .unwrap();
    let before = repository.list().await.unwrap();
    assert!(
        matches!(service.mount(definition("/mem/child", "local", local_root, true)).await,
        Err(VfsError::Mount(MountError::SubmountNotAllowed(path))) if path.as_str() == "/mem")
    );
    assert_eq!(repository.list().await.unwrap(), before);
    let missing_root = directory.path().join("missing-root");
    let disabled_child = definition("/mem/child", "local", missing_root.to_str().unwrap(), false);
    service.mount(disabled_child.clone()).await.unwrap();
    let mut enabled_child = disabled_child.clone();
    enabled_child.enable();
    assert!(matches!(
        service.mount(enabled_child).await,
        Err(VfsError::Mount(MountError::SubmountNotAllowed(_)))
    ));
    assert_eq!(
        repository.get(disabled_child.id()).await.unwrap(),
        Some(disabled_child)
    );

    service
        .mount(definition("/branch/leaf", "local", local_root, true))
        .await
        .unwrap();
    assert!(
        matches!(service.mount(definition("/branch", "memory", "/", true)).await,
        Err(VfsError::Mount(MountError::SubmountNotAllowed(path))) if path.as_str() == "/branch")
    );
    service
        .mount(definition("/branch", "local", local_root, true))
        .await
        .unwrap();
    service
        .unmount(VirtualPath::try_from("/branch").unwrap())
        .await
        .unwrap();
    let resolved = service
        .resolve_mount(&VirtualPath::try_from("/branch/leaf/file").unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resolved.mount().virtual_path().as_str(), "/branch/leaf");
    assert_eq!(resolved.relative_path().as_str(), "file");

    service
        .mount(definition("/disabled", "memory", "/", false))
        .await
        .unwrap();
    service
        .mount(definition("/disabled/child", "local", local_root, true))
        .await
        .unwrap();
    let before = repository.list().await.unwrap();
    assert!(matches!(
        service
            .mount(definition("/bad", "memory", "invalid-root", true))
            .await,
        Err(VfsError::Driver(DriverError::InvalidPath))
    ));
    assert!(matches!(
        service
            .mount(definition("/unknown", "unregistered", "", true))
            .await,
        Err(VfsError::Driver(DriverError::UnregisteredKind(_)))
    ));
    assert_eq!(repository.list().await.unwrap(), before);
    for enabled in [false, true] {
        assert!(matches!(
            service
                .mount(definition("/relative", "local", "relative-root", enabled))
                .await,
            Err(VfsError::Driver(DriverError::InvalidPath))
        ));
    }
    assert_eq!(repository.list().await.unwrap(), before);
    let offline = definition("/offline", "local", missing_root.to_str().unwrap(), false);
    service.mount(offline.clone()).await.unwrap();
    let mut enabled_offline = offline.clone();
    enabled_offline.enable();
    assert!(matches!(
        service.mount(enabled_offline).await,
        Err(VfsError::Driver(DriverError::NotFound))
    ));
    assert_eq!(repository.get(offline.id()).await.unwrap(), Some(offline));
}

#[tokio::test]
async fn independent_services_serialize_parent_and_child_mount_validation() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("mounts.sqlite");
    let (repository, first) = services(&file).await;
    let (_, second) = services(&file).await;
    let local_root = directory.path().to_str().unwrap();
    first
        .mount(definition("/", "local", local_root, true))
        .await
        .unwrap();
    let (parent, child) = tokio::join!(
        first.mount(definition("/race", "memory", "/", true)),
        second.mount(definition("/race/child", "local", local_root, true))
    );
    assert_ne!(parent.is_ok(), child.is_ok());
    let error = parent.err().or_else(|| child.err()).unwrap();
    assert!(matches!(
        error,
        VfsError::Mount(MountError::SubmountNotAllowed(_))
    ));
    assert_eq!(repository.list().await.unwrap().len(), 2);
}
