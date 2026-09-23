use std::fs;
use std::io::Read;

use asset_core::driver::Driver;
use asset_core::driver::error::DriverError;
use asset_core::entry::domain::EntryKind;
use asset_core::namespace::domain::{DriverPath, VirtualPath, VirtualRelativePath};
use asset_infra::driver::LocalDriver;

fn relative(value: &str) -> VirtualRelativePath {
    VirtualPath::try_from(value)
        .unwrap()
        .strip_prefix(&VirtualPath::root())
        .unwrap()
}

#[test]
fn lists_and_reads_files_below_a_bound_root() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("子目录")).unwrap();
    fs::write(root.path().join("z.txt"), b"hello").unwrap();
    fs::write(root.path().join("a.txt"), []).unwrap();
    fs::write(root.path().join("子目录").join("nested.txt"), b"nested").unwrap();

    let driver = LocalDriver::new();
    let backend = driver
        .bind(&DriverPath::new(root.path().to_str().unwrap()))
        .unwrap();
    let entries = backend.list(&relative("/")).unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].name().as_str(), "a.txt");
    assert_eq!(entries[0].kind(), EntryKind::File);
    assert_eq!(entries[0].size(), Some(0));
    assert_eq!(entries[1].name().as_str(), "z.txt");
    assert_eq!(entries[1].size(), Some(5));
    assert_eq!(entries[2].name().as_str(), "子目录");
    assert_eq!(entries[2].kind(), EntryKind::Directory);
    assert_eq!(entries[2].size(), None);

    let nested = backend.list(&relative("/子目录")).unwrap();
    assert_eq!(nested.len(), 1);
    assert_eq!(nested[0].name().as_str(), "nested.txt");

    let mut contents = String::new();
    backend
        .read(&relative("/子目录/nested.txt"))
        .unwrap()
        .read_to_string(&mut contents)
        .unwrap();
    assert_eq!(contents, "nested");
}

#[test]
fn reports_root_and_entry_errors() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("file"), b"x").unwrap();
    let driver = LocalDriver::new();

    assert!(matches!(
        driver.bind(&DriverPath::new("")),
        Err(DriverError::InvalidDriverPath)
    ));
    assert!(matches!(
        driver.bind(&DriverPath::new(
            root.path().join("missing").to_str().unwrap()
        )),
        Err(DriverError::NotFound)
    ));
    assert!(matches!(
        driver.bind(&DriverPath::new(root.path().join("file").to_str().unwrap())),
        Err(DriverError::NotDirectory)
    ));

    let backend = driver
        .bind(&DriverPath::new(root.path().to_str().unwrap()))
        .unwrap();
    assert!(matches!(
        backend.list(&relative("/file")),
        Err(DriverError::NotDirectory)
    ));
    assert!(matches!(
        backend.read(&relative("/")),
        Err(DriverError::IsDirectory)
    ));
    assert!(matches!(
        backend.read(&relative("/missing")),
        Err(DriverError::NotFound)
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
