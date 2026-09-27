use asset_infra::driver::MemoryDriver;
use asset_vfs::driver::{Driver, DriverError, DriverPath};
use asset_vfs::error::VfsError;
use asset_vfs::namespace::{VirtualPath, VirtualRelativePath};
use driver_conformance::Fixture;

fn path(value: &str) -> VirtualPath {
    VirtualPath::try_from(value).unwrap()
}

fn relative(value: &str) -> VirtualRelativePath {
    path(value).strip_prefix(&VirtualPath::root()).unwrap()
}

struct MemoryFixture {
    driver: MemoryDriver,
}

impl Fixture for MemoryFixture {
    fn new() -> Self {
        let driver = MemoryDriver::new();
        driver.create_directory(&path("/albums")).unwrap();
        driver.create_directory(&path("/albums/2026")).unwrap();
        driver.create_directory(&path("/albums/子目录")).unwrap();
        driver.create_directory(&path("/other")).unwrap();
        driver.insert_file(&path("/albums/a.txt"), []).unwrap();
        driver
            .insert_file(&path("/albums/z.txt"), b"hello")
            .unwrap();
        driver
            .insert_file(&path("/albums/2026/nested.txt"), b"nested")
            .unwrap();
        driver
            .insert_file(&path("/albums/子目录/文件.txt"), b"unicode")
            .unwrap();
        Self { driver }
    }

    fn driver(&self) -> &dyn Driver {
        &self.driver
    }

    fn root(&self) -> DriverPath {
        DriverPath::new("/albums")
    }

    fn other_root(&self) -> DriverPath {
        DriverPath::new("/other")
    }

    fn file_root(&self) -> DriverPath {
        DriverPath::new("/albums/a.txt")
    }

    fn missing_root(&self) -> DriverPath {
        DriverPath::new("/missing")
    }
}

driver_conformance::driver_conformance_tests!(MemoryFixture);

#[test]
fn clones_share_updates_with_existing_bound_backends() {
    let fixture = MemoryFixture::new();
    let backend = fixture.driver.bind(&fixture.root()).unwrap();
    let reader = backend.reader();
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
        driver.bind(&DriverPath::new("relative")),
        Err(VfsError::Driver(DriverError::InvalidPath))
    ));
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
