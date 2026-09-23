use std::fs;

use asset_core::driver::Driver;
use asset_core::driver::error::DriverError;
use asset_core::namespace::domain::{DriverPath, VirtualPath, VirtualRelativePath};
use asset_infra::driver::LocalDriver;
use driver_conformance::Fixture;

fn relative(value: &str) -> VirtualRelativePath {
    VirtualPath::try_from(value)
        .unwrap()
        .strip_prefix(&VirtualPath::root())
        .unwrap()
}

struct LocalFixture {
    directory: tempfile::TempDir,
    driver: LocalDriver,
}

impl Fixture for LocalFixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("albums");
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("2026")).unwrap();
        fs::create_dir(root.join("子目录")).unwrap();
        fs::create_dir(directory.path().join("other")).unwrap();
        fs::write(root.join("a.txt"), []).unwrap();
        fs::write(root.join("z.txt"), b"hello").unwrap();
        fs::write(root.join("2026").join("nested.txt"), b"nested").unwrap();
        fs::write(root.join("子目录").join("文件.txt"), b"unicode").unwrap();
        Self {
            directory,
            driver: LocalDriver::new(),
        }
    }

    fn driver(&self) -> &dyn Driver {
        &self.driver
    }

    fn root(&self) -> DriverPath {
        DriverPath::new(self.directory.path().join("albums").to_str().unwrap())
    }

    fn other_root(&self) -> DriverPath {
        DriverPath::new(self.directory.path().join("other").to_str().unwrap())
    }

    fn file_root(&self) -> DriverPath {
        DriverPath::new(self.directory.path().join("albums/a.txt").to_str().unwrap())
    }

    fn missing_root(&self) -> DriverPath {
        DriverPath::new(self.directory.path().join("missing").to_str().unwrap())
    }
}

driver_conformance::driver_conformance_tests!(LocalFixture);

#[test]
fn rejects_empty_root() {
    assert!(matches!(
        LocalDriver::new().bind(&DriverPath::new("")),
        Err(DriverError::InvalidDriverPath)
    ));
}

#[cfg(unix)]
#[test]
fn symlinks_outside_the_root_are_not_listed_or_readable() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("secret"), b"secret").unwrap();
    symlink(outside.path(), root.path().join("escape")).unwrap();
    symlink("escape/secret", root.path().join("shortcut")).unwrap();

    let backend = LocalDriver
        .bind(&DriverPath::new(root.path().to_str().unwrap()))
        .unwrap();
    assert!(backend.list(&relative("/")).unwrap().is_empty());
    assert!(backend.read(&relative("/escape/secret")).is_err());
    assert!(backend.read(&relative("/shortcut")).is_err());
    assert!(backend.list(&relative("/escape")).is_err());
}

#[cfg(unix)]
#[test]
fn rejects_names_that_the_virtual_namespace_cannot_represent() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join(OsStr::from_bytes(b"bad\xff")), b"x").unwrap();
    let backend = LocalDriver
        .bind(&DriverPath::new(root.path().to_str().unwrap()))
        .unwrap();
    assert!(matches!(
        backend.list(&relative("/")),
        Err(DriverError::UnrepresentableName)
    ));
}
