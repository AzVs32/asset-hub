use std::io::Read;

use asset_core::driver::Driver;
use asset_core::driver::error::DriverError;
use asset_core::entry::domain::EntryKind;
use asset_core::namespace::domain::{DriverPath, VirtualPath, VirtualRelativePath};
use asset_infra::driver::MemoryDriver;

fn path(value: &str) -> VirtualPath {
    VirtualPath::try_from(value).unwrap()
}

fn relative(value: &str) -> VirtualRelativePath {
    path(value).strip_prefix(&VirtualPath::root()).unwrap()
}

#[test]
fn lists_direct_children_in_name_order_with_metadata() {
    let driver = MemoryDriver::new();
    driver.create_directory(&path("/albums")).unwrap();
    driver.create_directory(&path("/albums/2026")).unwrap();
    driver
        .insert_file(&path("/albums/z.txt"), b"hello".to_vec())
        .unwrap();
    driver
        .insert_file(&path("/albums/a.txt"), Vec::new())
        .unwrap();
    driver
        .insert_file(&path("/albums/2026/hidden.txt"), b"x".to_vec())
        .unwrap();

    let backend = driver.bind(&DriverPath::new("/albums")).unwrap();
    let root_entries = backend.list(&relative("/")).unwrap();
    assert_eq!(root_entries.len(), 3);
    assert_eq!(root_entries[0].name().as_str(), "2026");
    assert_eq!(root_entries[0].kind(), EntryKind::Directory);
    assert_eq!(root_entries[0].size(), None);
    assert_eq!(root_entries[1].name().as_str(), "a.txt");
    assert_eq!(root_entries[1].size(), Some(0));
    assert_eq!(root_entries[2].name().as_str(), "z.txt");
    assert_eq!(root_entries[2].size(), Some(5));

    let nested = backend.list(&relative("/2026")).unwrap();
    assert_eq!(nested.len(), 1);
    assert_eq!(nested[0].name().as_str(), "hidden.txt");
}

#[test]
fn bound_roots_are_isolated_and_share_updates() {
    let driver = MemoryDriver::new();
    driver.create_directory(&path("/one")).unwrap();
    driver.create_directory(&path("/two")).unwrap();
    let one = driver.bind(&DriverPath::new("/one")).unwrap();
    let two = driver.bind(&DriverPath::new("/two")).unwrap();

    driver
        .clone()
        .insert_file(&path("/one/hello.txt"), b"hello".to_vec())
        .unwrap();
    assert_eq!(one.list(&relative("/")).unwrap().len(), 1);
    assert!(two.list(&relative("/")).unwrap().is_empty());
    assert!(matches!(
        two.read(&relative("/hello.txt")),
        Err(DriverError::NotFound)
    ));

    let mut contents = Vec::new();
    one.read(&relative("/hello.txt"))
        .unwrap()
        .read_to_end(&mut contents)
        .unwrap();
    assert_eq!(contents, b"hello");
}

#[test]
fn open_readers_keep_their_snapshot_after_file_replacement() {
    let driver = MemoryDriver::new();
    driver
        .insert_file(&path("/file"), b"before".to_vec())
        .unwrap();
    let backend = driver.bind(&DriverPath::new("/")).unwrap();
    let mut old_reader = backend.read(&relative("/file")).unwrap();

    driver
        .insert_file(&path("/file"), b"after".to_vec())
        .unwrap();
    let mut before = String::new();
    old_reader.read_to_string(&mut before).unwrap();
    let mut after = String::new();
    backend
        .read(&relative("/file"))
        .unwrap()
        .read_to_string(&mut after)
        .unwrap();
    assert_eq!(before, "before");
    assert_eq!(after, "after");
}

#[test]
fn rejects_invalid_roots_and_invalid_tree_operations() {
    let driver = MemoryDriver::new();
    driver.insert_file(&path("/file"), Vec::new()).unwrap();

    assert!(driver.bind(&DriverPath::new("")).is_ok());
    assert!(matches!(
        driver.bind(&DriverPath::new("relative")),
        Err(DriverError::InvalidDriverPath)
    ));
    assert!(matches!(
        driver.bind(&DriverPath::new("/missing")),
        Err(DriverError::NotFound)
    ));
    assert!(matches!(
        driver.bind(&DriverPath::new("/file")),
        Err(DriverError::NotDirectory)
    ));
    assert!(matches!(
        driver.create_directory(&path("/file/child")),
        Err(DriverError::NotDirectory)
    ));
    assert!(matches!(
        driver.insert_file(&path("/missing/child"), Vec::new()),
        Err(DriverError::NotFound)
    ));
    assert!(matches!(
        driver.insert_file(&path("/"), Vec::new()),
        Err(DriverError::IsDirectory)
    ));

    let backend = driver.bind(&DriverPath::new("/")).unwrap();
    assert!(matches!(
        backend.list(&relative("/file")),
        Err(DriverError::NotDirectory)
    ));
    assert!(matches!(
        backend.read(&relative("/")),
        Err(DriverError::IsDirectory)
    ));
    assert!(matches!(
        backend.list(&relative("/missing")),
        Err(DriverError::NotFound)
    ));
}
