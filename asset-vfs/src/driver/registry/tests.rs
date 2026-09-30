use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::DriverRegistry;
use crate::driver::{BoundDriver, Driver, DriverError, DriverKind, DriverPath};
use crate::error::VfsError;

struct TestDriver {
    kind: DriverKind,
    allows_submounts: bool,
    bind_calls: AtomicUsize,
}

impl TestDriver {
    fn new(kind: &str, allows_submounts: bool) -> Self {
        Self {
            kind: DriverKind::try_from(kind).unwrap(),
            allows_submounts,
            bind_calls: AtomicUsize::new(0),
        }
    }
}

impl Driver for TestDriver {
    fn kind(&self) -> DriverKind {
        self.kind.clone()
    }

    fn allows_submounts(&self) -> bool {
        self.allows_submounts
    }

    fn bind(&self, _root: &DriverPath) -> Result<Box<dyn BoundDriver>, VfsError> {
        self.bind_calls.fetch_add(1, Ordering::Relaxed);
        Err(DriverError::NotFound.into())
    }
}

#[test]
fn lookup_uses_declared_kinds_and_retains_shared_factories_without_binding() {
    let first = Arc::new(TestDriver::new("custom.allow", true));
    let second = Arc::new(TestDriver::new("custom.deny", false));
    let mut registry = DriverRegistry::new();
    registry.register(first.clone()).unwrap();
    registry.register(second.clone()).unwrap();

    let first_factory = registry.require(&first.kind()).unwrap();
    let second_factory = registry.get(&second.kind()).unwrap();
    assert!(first_factory.allows_submounts());
    assert!(!second_factory.allows_submounts());
    assert_eq!(first.bind_calls.load(Ordering::Relaxed), 0);
    assert_eq!(second.bind_calls.load(Ordering::Relaxed), 0);

    let registry = Arc::new(registry);
    let shared = Arc::clone(&registry);
    assert!(matches!(
        shared
            .get(&second.kind())
            .unwrap()
            .bind(&DriverPath::new("missing")),
        Err(VfsError::Driver(DriverError::NotFound))
    ));
    assert_eq!(first.bind_calls.load(Ordering::Relaxed), 0);
    assert_eq!(second.bind_calls.load(Ordering::Relaxed), 1);
}

#[test]
fn duplicate_registration_preserves_existing_and_other_factories() {
    let original = Arc::new(TestDriver::new("custom", false));
    let replacement = Arc::new(TestDriver::new("custom", true));
    let other = Arc::new(TestDriver::new("other", true));
    let mut registry = DriverRegistry::new();
    registry.register(original.clone()).unwrap();
    registry.register(other.clone()).unwrap();
    assert_eq!(registry.len(), 2);

    assert!(matches!(
        registry.register(replacement.clone()),
        Err(VfsError::Driver(DriverError::DuplicateKind(kind))) if kind == original.kind()
    ));
    assert_eq!(registry.len(), 2);
    assert!(!registry.get(&original.kind()).unwrap().allows_submounts());
    assert!(registry.get(&other.kind()).unwrap().allows_submounts());
    assert!(
        registry
            .get(&original.kind())
            .unwrap()
            .bind(&DriverPath::new("missing"))
            .is_err()
    );
    assert_eq!(original.bind_calls.load(Ordering::Relaxed), 1);
    assert_eq!(replacement.bind_calls.load(Ordering::Relaxed), 0);
    assert_eq!(other.bind_calls.load(Ordering::Relaxed), 0);
}

#[test]
fn unknown_kind_does_not_match_a_registered_prefix() {
    let driver = Arc::new(TestDriver::new("custom", true));
    let mut registry = DriverRegistry::default();
    assert!(registry.get(&driver.kind()).is_none());
    assert!(matches!(
        registry.require(&driver.kind()),
        Err(VfsError::Driver(DriverError::UnregisteredKind(kind))) if kind == driver.kind()
    ));
    registry.register(driver.clone()).unwrap();
    let unknown = DriverKind::try_from("custom.other").unwrap();
    assert!(registry.get(&unknown).is_none());
    assert!(matches!(
        registry.require(&unknown),
        Err(VfsError::Driver(DriverError::UnregisteredKind(kind))) if kind == unknown
    ));
    assert_eq!(driver.bind_calls.load(Ordering::Relaxed), 0);
}

#[test]
fn listing_is_sorted_and_returns_independent_snapshots_without_binding() {
    let drivers = ["z", "a.child", "a"].map(|kind| Arc::new(TestDriver::new(kind, true)));
    let mut registry = DriverRegistry::new();
    assert!(registry.list().is_empty());
    assert_eq!(registry.len(), 0);
    for driver in &drivers {
        registry.register(driver.clone()).unwrap();
    }
    assert_eq!(registry.len(), 3);
    let snapshot = registry.list();
    assert_eq!(
        snapshot.iter().map(DriverKind::as_str).collect::<Vec<_>>(),
        ["a", "a.child", "z"]
    );

    registry
        .register(Arc::new(TestDriver::new("b", false)))
        .unwrap();
    assert_eq!(registry.len(), 4);
    assert_eq!(
        registry
            .list()
            .iter()
            .map(DriverKind::as_str)
            .collect::<Vec<_>>(),
        ["a", "a.child", "b", "z"]
    );
    assert_eq!(
        snapshot.iter().map(DriverKind::as_str).collect::<Vec<_>>(),
        ["a", "a.child", "z"]
    );
    for driver in drivers {
        assert_eq!(driver.bind_calls.load(Ordering::Relaxed), 0);
    }
}
