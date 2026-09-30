use std::fs;
use std::path::Path;

use asset_infra::driver::LocalDriver;
use asset_vfs::driver::{BoundDriver, Driver, DriverError, DriverPath, ReadDriver};
use asset_vfs::error::VfsError;
use asset_vfs::namespace::{VirtualPath, VirtualRelativePath};
use asset_vfs_conformance::driver::data::TreeBuilder;
use asset_vfs_conformance::driver::{binding, bound_driver, read_driver};
use binding::Fixture as _;
use bound_driver::Fixture as _;
use read_driver::Fixture as _;

fn relative(value: &str) -> VirtualRelativePath {
    VirtualPath::try_from(value)
        .unwrap()
        .strip_prefix(&VirtualPath::root())
        .unwrap()
}

// One native adapter prepares every standard conformance tree.
struct LocalTreeBuilder<'a> {
    root: &'a Path,
}

impl TreeBuilder for LocalTreeBuilder<'_> {
    type Error = std::io::Error;

    fn create_directory(&mut self, path: &VirtualRelativePath) -> Result<(), Self::Error> {
        fs::create_dir_all(self.root.join(path.as_str()))
    }

    fn write_file(
        &mut self,
        path: &VirtualRelativePath,
        contents: &[u8],
    ) -> Result<(), Self::Error> {
        fs::write(self.root.join(path.as_str()), contents)
    }
}

struct LocalDriverFixture {
    directory: tempfile::TempDir,
    driver: LocalDriver,
}

impl binding::Fixture for LocalDriverFixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        binding::TREE
            .populate(&mut LocalTreeBuilder {
                root: &directory.path().join("albums"),
            })
            .unwrap();
        Self {
            directory,
            driver: LocalDriver::new(),
        }
    }

    fn driver(&self) -> &dyn Driver {
        &self.driver
    }

    fn driver_path(&self, path: &VirtualRelativePath) -> DriverPath {
        DriverPath::new(
            self.directory
                .path()
                .join("albums")
                .join(path.as_str())
                .to_str()
                .unwrap(),
        )
    }
}

struct LocalBoundFixture {
    backend: Box<dyn BoundDriver>,
    other_backend: Box<dyn BoundDriver>,
    _directory: tempfile::TempDir,
}

impl bound_driver::Fixture for LocalBoundFixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("albums");
        let other = directory.path().join("other");
        bound_driver::TREE
            .populate(&mut LocalTreeBuilder { root: &root })
            .unwrap();
        bound_driver::OTHER_TREE
            .populate(&mut LocalTreeBuilder { root: &other })
            .unwrap();
        let driver = LocalDriver::new();
        let backend = driver
            .bind(&DriverPath::new(root.to_str().unwrap()))
            .unwrap();
        let other_backend = driver
            .bind(&DriverPath::new(other.to_str().unwrap()))
            .unwrap();
        Self {
            backend,
            other_backend,
            _directory: directory,
        }
    }

    fn backend(&self) -> &dyn BoundDriver {
        self.backend.as_ref()
    }

    fn other_backend(&self) -> &dyn BoundDriver {
        self.other_backend.as_ref()
    }
}

struct LocalReadFixture {
    backend: Box<dyn BoundDriver>,
    _directory: tempfile::TempDir,
}

impl read_driver::Fixture for LocalReadFixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("albums");
        read_driver::TREE
            .populate(&mut LocalTreeBuilder { root: &root })
            .unwrap();
        let backend = LocalDriver::new()
            .bind(&DriverPath::new(root.to_str().unwrap()))
            .unwrap();
        Self {
            backend,
            _directory: directory,
        }
    }

    fn reader(&self) -> &dyn ReadDriver {
        self.backend.reader()
    }
}

mod conformance {
    use super::*;

    mod driver {
        use super::*;

        #[test]
        fn bind_directory() {
            binding::check_bind_directory(&LocalDriverFixture::new());
        }

        #[test]
        fn bind_missing_root() {
            binding::check_bind_missing_root(&LocalDriverFixture::new());
        }

        #[test]
        fn bind_file_root() {
            binding::check_bind_file_root(&LocalDriverFixture::new());
        }
    }

    mod bound {
        use super::*;

        #[test]
        fn reader_uses_bound_root() {
            bound_driver::check_reader_uses_bound_root(&LocalBoundFixture::new());
        }

        #[test]
        fn bindings_are_independent() {
            bound_driver::check_bindings_are_independent(&LocalBoundFixture::new());
        }

        #[test]
        fn writer_is_not_exposed() {
            bound_driver::check_writer_presence(&LocalBoundFixture::new(), false);
        }
    }

    mod read {
        use super::*;

        #[test]
        fn direct_children() {
            read_driver::check_direct_children(&LocalReadFixture::new());
        }

        #[test]
        fn listing_order() {
            read_driver::check_listing_order(&LocalReadFixture::new());
        }

        #[test]
        fn entry_metadata() {
            read_driver::check_entry_metadata(&LocalReadFixture::new());
        }

        #[test]
        fn nested_listing() {
            read_driver::check_nested_listing(&LocalReadFixture::new());
        }

        #[test]
        fn empty_directory() {
            read_driver::check_empty_directory(&LocalReadFixture::new());
        }

        #[test]
        fn list_file() {
            read_driver::check_list_file(&LocalReadFixture::new());
        }

        #[test]
        fn list_missing() {
            read_driver::check_list_missing(&LocalReadFixture::new());
        }

        #[test]
        fn list_beneath_file() {
            read_driver::check_list_beneath_file(&LocalReadFixture::new());
        }
    }
}

#[test]
fn rejects_empty_and_relative_roots() {
    let fixture = LocalDriverFixture::new();
    for path in ["", ".", "..", "data", "./data", "../data"] {
        let root = DriverPath::new(path);
        assert!(matches!(
            fixture.driver().validate_path(&root),
            Err(VfsError::Driver(DriverError::InvalidPath))
        ));
        binding::check_bind_invalid_root(&fixture, &root);
    }
}

#[test]
fn absolute_path_validation_does_not_require_an_existing_directory() {
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join("missing");
    let root = DriverPath::new(missing.to_str().unwrap());
    LocalDriver.validate_path(&root).unwrap();
    assert!(!missing.exists());
    assert!(matches!(
        LocalDriver.bind(&root),
        Err(VfsError::Driver(DriverError::NotFound))
    ));
}

#[cfg(unix)]
#[test]
fn symlinks_outside_the_root_are_not_listed_or_traversed() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("secret"), b"secret").unwrap();
    symlink(outside.path(), root.path().join("escape")).unwrap();
    symlink("escape/secret", root.path().join("shortcut")).unwrap();

    let backend = LocalDriver
        .bind(&DriverPath::new(root.path().to_str().unwrap()))
        .unwrap();
    let reader = backend.reader();
    assert!(reader.list(&relative("/")).unwrap().is_empty());
    assert!(reader.list(&relative("/escape")).is_err());
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
    let reader = backend.reader();
    assert!(matches!(
        reader.list(&relative("/")),
        Err(VfsError::Driver(DriverError::UnrepresentableName))
    ));
}
