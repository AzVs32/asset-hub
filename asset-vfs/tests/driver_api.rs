use std::error::Error;
use std::io;

use asset_vfs::driver::{BoundDriver, Driver, DriverError, DriverPath, ReadDriver, WriteDriver};
use asset_vfs::entry::Entry;
use asset_vfs::error::VfsError;
use asset_vfs::namespace::{VirtualPath, VirtualRelativePath};

struct MissingDriver;

impl Driver for MissingDriver {
    fn bind(&self, _root: &DriverPath) -> Result<Box<dyn BoundDriver>, VfsError> {
        Err(DriverError::NotFound.into())
    }
}

#[test]
fn external_drivers_construct_and_match_contract_errors() {
    let driver: Box<dyn Driver> = Box::new(MissingDriver);
    let Err(error) = driver.bind(&DriverPath::new("missing")) else {
        panic!("expected a missing root error");
    };
    match error {
        VfsError::Driver(DriverError::NotFound) => {}
        other => panic!("expected DriverError::NotFound, got {other:?}"),
    }
}

#[test]
fn backend_failure_preserves_the_original_source_through_component_wrapping() {
    let error: VfsError =
        DriverError::backend(io::Error::new(io::ErrorKind::PermissionDenied, "denied")).into();
    assert!(matches!(error, VfsError::Driver(_)));
    let component = error.source().unwrap();
    let original = component
        .source()
        .unwrap()
        .downcast_ref::<io::Error>()
        .unwrap();
    assert_eq!(original.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(original.to_string(), "denied");
    assert!(error.to_string().contains("denied"));
}

struct WritableBackend;

impl ReadDriver for WritableBackend {
    fn list(&self, _path: &VirtualRelativePath) -> Result<Vec<Entry>, VfsError> {
        Ok(Vec::new())
    }
}

impl WriteDriver for WritableBackend {}

impl BoundDriver for WritableBackend {
    fn reader(&self) -> &dyn ReadDriver {
        self
    }

    fn as_writer(&self) -> Option<&dyn WriteDriver> {
        Some(self)
    }
}

#[test]
fn external_backends_expose_required_read_and_optional_write_interfaces() {
    let backend: Box<dyn BoundDriver> = Box::new(WritableBackend);
    let root = VirtualPath::root();
    let relative_root = root.strip_prefix(&root).unwrap();

    assert!(backend.reader().list(&relative_root).unwrap().is_empty());
    assert!(backend.as_writer().is_some());
}
