use std::io::{Cursor, Read};

use crate::driver::{DriverError, DriverPath};
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
    fn list(&self, _path: &VirtualRelativePath) -> Result<Vec<Entry>, VfsError> {
        Ok(vec![Entry::file(
            EntryName::try_from("a.txt").unwrap(),
            Some(5),
        )])
    }

    fn read(
        &self,
        _path: &VirtualRelativePath,
    ) -> Result<Box<dyn Read + Send + 'static>, VfsError> {
        Ok(Box::new(Cursor::new(b"hello".to_vec())))
    }
}

fn relative_path(value: &str) -> VirtualRelativePath {
    let root = VirtualPath::root();
    VirtualPath::try_from(format!("/{value}").as_str())
        .unwrap()
        .strip_prefix(&root)
        .unwrap()
}

#[test]
fn driver_binds_once_and_exposes_object_safe_read_operations() {
    let driver: Box<dyn Driver> = Box::new(TestDriver);
    let backend = driver.bind(&DriverPath::new("root")).unwrap();
    let path = relative_path("a.txt");

    let root = VirtualPath::root();
    let relative_root = root.strip_prefix(&root).unwrap();
    let entries = backend.list(&relative_root).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name().as_str(), "a.txt");
    assert_eq!(entries[0].kind(), EntryKind::File);

    let mut contents = String::new();
    backend
        .read(&path)
        .unwrap()
        .read_to_string(&mut contents)
        .unwrap();
    assert_eq!(contents, "hello");
}

#[test]
fn driver_rejects_an_invalid_root_during_binding() {
    let Err(error) = TestDriver.bind(&DriverPath::new("invalid")) else {
        panic!("expected binding to reject the driver path");
    };

    assert!(matches!(error, VfsError::Driver(DriverError::InvalidPath)));
}
