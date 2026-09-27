use asset_infra::driver::MemoryDriver;
use asset_vfs::driver::{BoundDriver, Driver, DriverError, DriverPath, ReadDriver};
use asset_vfs::error::VfsError;
use asset_vfs::namespace::{VirtualPath, VirtualRelativePath};
use asset_vfs_conformance::driver::data::TreeBuilder;
use asset_vfs_conformance::driver::{binding, bound_driver, read_driver};
use binding::Fixture as _;
use bound_driver::Fixture as _;
use read_driver::Fixture as _;

fn path(value: &str) -> VirtualPath {
    VirtualPath::try_from(value).unwrap()
}

fn relative(value: &str) -> VirtualRelativePath {
    path(value).strip_prefix(&VirtualPath::root()).unwrap()
}

fn resolve(root: &VirtualPath, path: &VirtualRelativePath) -> VirtualPath {
    let mut resolved = root.clone();
    for segment in path
        .as_str()
        .split('/')
        .filter(|segment| !segment.is_empty())
    {
        resolved = resolved.join_segment(segment).unwrap();
    }
    resolved
}

// The same two operations prepare binding, isolation, and listing data.
struct MemoryTreeBuilder<'a> {
    driver: &'a MemoryDriver,
    root: VirtualPath,
}

impl TreeBuilder for MemoryTreeBuilder<'_> {
    type Error = VfsError;

    fn create_directory(&mut self, path: &VirtualRelativePath) -> Result<(), Self::Error> {
        self.driver.create_directory(&resolve(&self.root, path))
    }

    fn write_file(
        &mut self,
        path: &VirtualRelativePath,
        contents: &[u8],
    ) -> Result<(), Self::Error> {
        self.driver
            .insert_file(&resolve(&self.root, path), contents)
    }
}

struct MemoryDriverFixture {
    driver: MemoryDriver,
}

impl binding::Fixture for MemoryDriverFixture {
    fn new() -> Self {
        let driver = MemoryDriver::new();
        binding::TREE
            .populate(&mut MemoryTreeBuilder {
                driver: &driver,
                root: path("/albums"),
            })
            .unwrap();
        Self { driver }
    }

    fn driver(&self) -> &dyn Driver {
        &self.driver
    }

    fn driver_path(&self, path: &VirtualRelativePath) -> DriverPath {
        DriverPath::new(resolve(&VirtualPath::try_from("/albums").unwrap(), path).as_str())
    }
}

struct MemoryBoundFixture {
    backend: Box<dyn BoundDriver>,
    other_backend: Box<dyn BoundDriver>,
}

impl bound_driver::Fixture for MemoryBoundFixture {
    fn new() -> Self {
        let driver = MemoryDriver::new();
        bound_driver::TREE
            .populate(&mut MemoryTreeBuilder {
                driver: &driver,
                root: path("/albums"),
            })
            .unwrap();
        bound_driver::OTHER_TREE
            .populate(&mut MemoryTreeBuilder {
                driver: &driver,
                root: path("/other"),
            })
            .unwrap();
        let backend = driver.bind(&DriverPath::new("/albums")).unwrap();
        let other_backend = driver.bind(&DriverPath::new("/other")).unwrap();
        Self {
            backend,
            other_backend,
        }
    }

    fn backend(&self) -> &dyn BoundDriver {
        self.backend.as_ref()
    }

    fn other_backend(&self) -> &dyn BoundDriver {
        self.other_backend.as_ref()
    }
}

struct MemoryReadFixture {
    driver: MemoryDriver,
    backend: Box<dyn BoundDriver>,
}

impl read_driver::Fixture for MemoryReadFixture {
    fn new() -> Self {
        let driver = MemoryDriver::new();
        read_driver::TREE
            .populate(&mut MemoryTreeBuilder {
                driver: &driver,
                root: path("/albums"),
            })
            .unwrap();
        let backend = driver.bind(&DriverPath::new("/albums")).unwrap();
        Self { driver, backend }
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
            binding::check_bind_directory(&MemoryDriverFixture::new());
        }

        #[test]
        fn bind_missing_root() {
            binding::check_bind_missing_root(&MemoryDriverFixture::new());
        }

        #[test]
        fn bind_file_root() {
            binding::check_bind_file_root(&MemoryDriverFixture::new());
        }

        #[test]
        fn bind_invalid_root() {
            binding::check_bind_invalid_root(
                &MemoryDriverFixture::new(),
                &DriverPath::new("relative"),
            );
        }
    }

    mod bound {
        use super::*;

        #[test]
        fn reader_uses_bound_root() {
            bound_driver::check_reader_uses_bound_root(&MemoryBoundFixture::new());
        }

        #[test]
        fn bindings_are_independent() {
            bound_driver::check_bindings_are_independent(&MemoryBoundFixture::new());
        }

        #[test]
        fn writer_is_not_exposed() {
            bound_driver::check_writer_presence(&MemoryBoundFixture::new(), false);
        }
    }

    mod read {
        use super::*;

        #[test]
        fn direct_children() {
            read_driver::check_direct_children(&MemoryReadFixture::new());
        }

        #[test]
        fn listing_order() {
            read_driver::check_listing_order(&MemoryReadFixture::new());
        }

        #[test]
        fn entry_metadata() {
            read_driver::check_entry_metadata(&MemoryReadFixture::new());
        }

        #[test]
        fn nested_listing() {
            read_driver::check_nested_listing(&MemoryReadFixture::new());
        }

        #[test]
        fn empty_directory() {
            read_driver::check_empty_directory(&MemoryReadFixture::new());
        }

        #[test]
        fn list_file() {
            read_driver::check_list_file(&MemoryReadFixture::new());
        }

        #[test]
        fn list_missing() {
            read_driver::check_list_missing(&MemoryReadFixture::new());
        }

        #[test]
        fn list_beneath_file() {
            read_driver::check_list_beneath_file(&MemoryReadFixture::new());
        }
    }
}

#[test]
fn clones_share_updates_with_existing_bound_backends() {
    let fixture = MemoryReadFixture::new();
    let reader = fixture.backend.reader();
    fixture
        .driver
        .clone()
        .insert_file(&path("/albums/new.txt"), b"new")
        .unwrap();

    let entries = reader.list(&relative("/")).unwrap();
    let new_file = entries
        .iter()
        .find(|entry| entry.name().as_str() == "new.txt")
        .expect("existing bound backends must observe shared tree updates");
    assert_eq!(new_file.size(), Some(3));
}

#[test]
fn rejects_invalid_tree_operations_and_supports_empty_root_alias() {
    let driver = MemoryDriver::new();
    driver.insert_file(&path("/file"), Vec::new()).unwrap();

    assert!(driver.bind(&DriverPath::new("")).is_ok());
    assert!(matches!(
        driver.create_directory(&path("/file/child")),
        Err(VfsError::Driver(DriverError::NotDirectory))
    ));
    assert!(matches!(
        driver.insert_file(&path("/missing/child"), Vec::new()),
        Err(VfsError::Driver(DriverError::NotFound))
    ));
    assert!(matches!(
        driver.insert_file(&path("/"), Vec::new()),
        Err(VfsError::Driver(DriverError::IsDirectory))
    ));
}
