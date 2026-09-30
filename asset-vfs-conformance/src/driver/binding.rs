//! Root-binding checks for [`Driver`].

use asset_vfs::driver::{Driver, DriverError, DriverPath};
use asset_vfs::error::VfsError;

pub use super::data::BINDING_TREE as TREE;
use super::data::{missing_child, root_path};
use asset_vfs::entry::EntryKind;
use asset_vfs::namespace::VirtualRelativePath;

/// Owns a driver and a native root populated from [`TREE`].
///
/// Implement `driver_path` to map canonical relative paths to native locations.
/// The standard directory, file, and missing-root cases are derived automatically.
/// The driver's own root-path syntax remains implementation-specific.
pub trait Fixture: Sized {
    fn new() -> Self;
    fn driver(&self) -> &dyn Driver;
    fn driver_path(&self, path: &VirtualRelativePath) -> DriverPath;

    fn root(&self) -> DriverPath {
        self.driver_path(&root_path())
    }

    fn file_root(&self) -> DriverPath {
        let file = TREE
            .entries()
            .iter()
            .find(|entry| entry.kind() == EntryKind::File)
            .expect("the binding tree must contain a file");
        self.driver_path(&file.path())
    }

    fn missing_root(&self) -> DriverPath {
        self.driver_path(&missing_child(&root_path()))
    }
}

/// Checks that an existing directory can be bound.
pub fn check_bind_directory(fixture: &impl Fixture) {
    fixture
        .driver()
        .validate_path(&fixture.root())
        .expect("an existing directory root must have valid syntax");
    fixture
        .driver()
        .bind(&fixture.root())
        .expect("binding an existing directory must succeed");
}

/// Checks that binding an absent root reports `NotFound`.
pub fn check_bind_missing_root(fixture: &impl Fixture) {
    fixture
        .driver()
        .validate_path(&fixture.missing_root())
        .expect("a missing root must still have valid syntax");
    assert!(
        matches!(
            fixture.driver().bind(&fixture.missing_root()),
            Err(VfsError::Driver(DriverError::NotFound))
        ),
        "binding a missing root must return DriverError::NotFound"
    );
}

/// Checks that binding a file reports `NotDirectory`.
pub fn check_bind_file_root(fixture: &impl Fixture) {
    fixture
        .driver()
        .validate_path(&fixture.file_root())
        .expect("a file root must still have valid syntax");
    assert!(
        matches!(
            fixture.driver().bind(&fixture.file_root()),
            Err(VfsError::Driver(DriverError::NotDirectory))
        ),
        "binding a file must return DriverError::NotDirectory"
    );
}

/// Checks a root that the caller explicitly identifies as syntactically invalid.
///
/// This check is opt-in: no string is assumed to be invalid for every driver.
pub fn check_bind_invalid_root(fixture: &impl Fixture, invalid_root: &DriverPath) {
    assert!(
        matches!(
            fixture.driver().validate_path(invalid_root),
            Err(VfsError::Driver(DriverError::InvalidPath))
        ),
        "validating an invalid root must return DriverError::InvalidPath"
    );
    assert!(
        matches!(
            fixture.driver().bind(invalid_root),
            Err(VfsError::Driver(DriverError::InvalidPath))
        ),
        "binding an invalid root must return DriverError::InvalidPath"
    );
}
