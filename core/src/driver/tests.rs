use std::io::{Cursor, Read};

use crate::driver::domain::DriverKind;
use crate::driver::error::{DriverError, DriverKindError};
use crate::entry::domain::{Entry, EntryKind, Metadata};
use crate::namespace::domain::{DriverPath, EntryName, VirtualPath, VirtualRelativePath};

use super::{BoundDriver, Driver};

struct TestDriver;

impl Driver for TestDriver {
    fn bind(&self, root: &DriverPath) -> Result<Box<dyn BoundDriver>, DriverError> {
        if root.as_str() == "invalid" {
            return Err(DriverError::InvalidDriverPath);
        }

        Ok(Box::new(TestBackend))
    }
}

struct TestBackend;

impl BoundDriver for TestBackend {
    fn stat(&self, _path: &VirtualRelativePath) -> Result<Metadata, DriverError> {
        Ok(Metadata::file(Some(5)))
    }

    fn list(&self, _path: &VirtualRelativePath) -> Result<Vec<Entry>, DriverError> {
        Ok(vec![Entry::file(
            EntryName::try_from("a.txt").unwrap(),
            Some(5),
        )])
    }

    fn read(
        &self,
        _path: &VirtualRelativePath,
    ) -> Result<Box<dyn Read + Send + 'static>, DriverError> {
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
fn driver_kind_is_a_stable_registry_identifier() {
    let kind = DriverKind::try_from("memory.v1").unwrap();

    assert_eq!(kind.as_str(), "memory.v1");
    assert_eq!(kind.as_ref(), "memory.v1");
    assert_eq!(kind.to_string(), "memory.v1");
    assert_eq!(
        DriverKind::try_from("Memory").unwrap_err(),
        DriverKindError::InvalidCharacter { character: 'M' }
    );
}

#[test]
fn driver_kind_rejects_empty_and_oversized_identifiers() {
    assert_eq!(
        DriverKind::try_from("").unwrap_err(),
        DriverKindError::Empty
    );

    let oversized = "a".repeat(65);
    assert_eq!(
        DriverKind::try_from(oversized.as_str()).unwrap_err(),
        DriverKindError::TooLong {
            length: 65,
            max: 64,
        }
    );
}

#[test]
fn driver_binds_once_and_exposes_object_safe_read_operations() {
    let driver: Box<dyn Driver> = Box::new(TestDriver);
    let backend = driver.bind(&DriverPath::new("root")).unwrap();
    let path = relative_path("a.txt");

    assert_eq!(backend.stat(&path).unwrap(), Metadata::file(Some(5)));

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

    assert!(matches!(error, DriverError::InvalidDriverPath));
}
