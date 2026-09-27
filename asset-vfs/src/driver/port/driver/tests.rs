use crate::driver::{DriverError, DriverPath, ReadDriver};
use crate::entry::{Entry, EntryKind};
use crate::error::VfsError;
use crate::namespace::{EntryName, VirtualPath, VirtualRelativePath};

use super::{BoundDriver, Driver};

struct TestDriver;

impl Driver for TestDriver {
    fn bind(&self, root: &DriverPath) -> Result<Box<dyn BoundDriver>, VfsError> {
        if root.as_str() == "invalid" {
            return Err(DriverError::InvalidPath.into());
        }

        Ok(Box::new(TestBackend))
    }
}

struct TestBackend;

impl BoundDriver for TestBackend {
    fn reader(&self) -> &dyn ReadDriver {
        self
    }
}

impl ReadDriver for TestBackend {
    fn list(&self, _path: &VirtualRelativePath) -> Result<Vec<Entry>, VfsError> {
        Ok(vec![Entry::file(
            EntryName::try_from("a.txt").unwrap(),
            Some(5),
        )])
    }
}

#[test]
fn driver_binds_once_and_exposes_object_safe_listing() {
    let driver: Box<dyn Driver> = Box::new(TestDriver);
    let backend = driver.bind(&DriverPath::new("root")).unwrap();

    let root = VirtualPath::root();
    let relative_root = root.strip_prefix(&root).unwrap();
    assert!(backend.as_writer().is_none());
    let entries = backend.reader().list(&relative_root).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name().as_str(), "a.txt");
    assert_eq!(entries[0].kind(), EntryKind::File);

    assert_eq!(entries[0].size(), Some(5));
}

#[test]
fn driver_rejects_an_invalid_root_during_binding() {
    let Err(error) = TestDriver.bind(&DriverPath::new("invalid")) else {
        panic!("expected binding to reject the driver path");
    };

    assert!(matches!(error, VfsError::Driver(DriverError::InvalidPath)));
}
