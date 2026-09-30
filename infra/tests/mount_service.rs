use std::sync::Arc;

use asset_infra::repository::SqliteMountRepository;
use asset_vfs::driver::{BoundDriver, Driver, DriverKind, DriverPath};
use asset_vfs::error::VfsError;
use asset_vfs::mount::{Mount, MountId, MountRepository};
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
}
