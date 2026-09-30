use std::sync::Arc;

use asset_vfs::DriverService;
use asset_vfs::driver::{BoundDriver, Driver, DriverError, DriverKind, DriverPath};
use asset_vfs::error::VfsError;

struct MetadataOnlyDriver {
    kind: DriverKind,
    allows_submounts: bool,
}

impl Driver for MetadataOnlyDriver {
    fn kind(&self) -> DriverKind {
        self.kind.clone()
    }

    fn allows_submounts(&self) -> bool {
        self.allows_submounts
    }

    fn validate_path(&self, _root: &DriverPath) -> Result<(), VfsError> {
        panic!("driver metadata queries must not validate backend paths")
    }

    fn bind(&self, _root: &DriverPath) -> Result<Box<dyn BoundDriver>, VfsError> {
        panic!("driver service must not bind backends")
    }
}

fn factory(kind: &str, allows_submounts: bool) -> Arc<dyn Driver> {
    Arc::new(MetadataOnlyDriver {
        kind: DriverKind::try_from(kind).unwrap(),
        allows_submounts,
    })
}

#[test]
fn public_queries_return_sorted_kinds_and_metadata_without_binding() {
    let service =
        DriverService::new([factory("custom.deny", false), factory("custom.allow", true)]).unwrap();
    assert_eq!(service.len(), 2);
    assert!(!service.is_empty());
    let mut kinds = service.list();
    assert_eq!(
        kinds.iter().map(DriverKind::as_str).collect::<Vec<_>>(),
        ["custom.allow", "custom.deny"]
    );
    for (kind, policy) in kinds.iter().zip([true, false]) {
        let info = service.info(kind).unwrap();
        assert_eq!(info.kind(), kind);
        assert_eq!(info.allows_submounts(), policy);
        assert_eq!(service.require(kind).unwrap(), info);
    }
    kinds.clear();
    assert_eq!(service.list().len(), 2);

    let unknown = DriverKind::try_from("custom.allow.other").unwrap();
    assert!(service.info(&unknown).is_none());
    assert!(matches!(
        service.require(&unknown),
        Err(VfsError::Driver(DriverError::UnregisteredKind(kind))) if kind == unknown
    ));
}

#[test]
fn services_own_independent_registrations_and_allow_an_empty_set() {
    let empty = DriverService::new([]).unwrap();
    let allow = DriverService::new([factory("custom", true)]).unwrap();
    let deny = DriverService::new([factory("custom", false)]).unwrap();
    let kind = DriverKind::try_from("custom").unwrap();
    assert!(empty.is_empty());
    assert_eq!(empty.len(), 0);
    assert!(empty.list().is_empty());
    assert!(empty.info(&kind).is_none());
    assert!(allow.require(&kind).unwrap().allows_submounts());
    assert!(!deny.require(&kind).unwrap().allows_submounts());
}

#[test]
fn construction_rejects_duplicate_kinds_without_binding() {
    let kind = DriverKind::try_from("custom").unwrap();
    assert!(matches!(
        DriverService::new([factory("custom", true), factory("custom", false)]),
        Err(VfsError::Driver(DriverError::DuplicateKind(duplicate))) if duplicate == kind
    ));
}
