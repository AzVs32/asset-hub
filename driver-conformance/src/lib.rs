//! Shared behavioral checks for concrete [`Driver`] implementations.
//!
//! Each fixture must create this tree under its bound root:
//!
//! ```text
//! /
//! ├── 2026/
//! │   └── nested.txt  ("nested")
//! ├── a.txt           (empty)
//! ├── z.txt           ("hello")
//! └── 子目录/
//!     └── 文件.txt    ("unicode")
//! ```
//!
//! `other_root` is an empty sibling directory. `file_root` points to a file,
//! and `missing_root` points to a nonexistent path. The fixture owns any
//! temporary resources for the duration of each check.

use std::io::Read;

use asset_core::domain::{DriverPath, VirtualPath, VirtualRelativePath};
use asset_core::domain::{Entry, EntryKind};
use asset_core::port::Driver;

/// Supplies the same directory tree using a concrete driver's native storage.
pub trait Fixture: Sized {
    fn new() -> Self;
    fn driver(&self) -> &dyn Driver;
    fn root(&self) -> DriverPath;
    fn other_root(&self) -> DriverPath;
    fn file_root(&self) -> DriverPath;
    fn missing_root(&self) -> DriverPath;
}

fn relative(value: &str) -> VirtualRelativePath {
    VirtualPath::try_from(value)
        .unwrap()
        .strip_prefix(&VirtualPath::root())
        .unwrap()
}

fn assert_file(entry: &Entry, name: &str, size: u64) {
    assert_eq!(entry.name().as_str(), name);
    assert_eq!(entry.kind(), EntryKind::File);
    assert!(entry.size().is_none_or(|actual| actual == size));
}

/// Checks direct children, ordering, directory metadata, and nested listing.
pub fn check_listing(fixture: &impl Fixture) {
    let backend = fixture.driver().bind(&fixture.root()).unwrap();
    let entries = backend.list(&relative("/")).unwrap();
    assert_eq!(entries.len(), 4);
    assert_eq!(entries[0].name().as_str(), "2026");
    assert_eq!(entries[0].kind(), EntryKind::Directory);
    assert_eq!(entries[0].size(), None);
    assert_file(&entries[1], "a.txt", 0);
    assert_file(&entries[2], "z.txt", 5);
    assert_eq!(entries[3].name().as_str(), "子目录");
    assert_eq!(entries[3].kind(), EntryKind::Directory);
    assert_eq!(entries[3].size(), None);

    let nested = backend.list(&relative("/2026")).unwrap();
    assert_eq!(nested.len(), 1);
    assert_file(&nested[0], "nested.txt", 6);

    let unicode = backend.list(&relative("/子目录")).unwrap();
    assert_eq!(unicode.len(), 1);
    assert_file(&unicode[0], "文件.txt", 7);
}

/// Checks reads from the root and a nested directory.
pub fn check_reading(fixture: &impl Fixture) {
    let backend = fixture.driver().bind(&fixture.root()).unwrap();
    for (path, expected) in [
        ("/a.txt", &b""[..]),
        ("/z.txt", &b"hello"[..]),
        ("/2026/nested.txt", &b"nested"[..]),
        ("/子目录/文件.txt", &b"unicode"[..]),
    ] {
        let mut contents = Vec::new();
        backend
            .read(&relative(path))
            .unwrap()
            .read_to_end(&mut contents)
            .unwrap();
        assert_eq!(contents, expected, "read {path}");
    }
}

/// Checks that a bound backend cannot see a sibling root's contents.
pub fn check_root_isolation(fixture: &impl Fixture) {
    let root = fixture.driver().bind(&fixture.root()).unwrap();
    let other = fixture.driver().bind(&fixture.other_root()).unwrap();
    assert_eq!(root.list(&relative("/")).unwrap().len(), 4);
    assert!(other.list(&relative("/")).unwrap().is_empty());
    assert!(matches!(
        other.read(&relative("/z.txt")),
        Err(error) if error.is_not_found()
    ));
}

/// Checks errors shared by all drivers for missing and non-directory paths.
pub fn check_errors(fixture: &impl Fixture) {
    assert!(matches!(
        fixture.driver().bind(&fixture.missing_root()),
        Err(error) if error.is_not_found()
    ));
    assert!(matches!(
        fixture.driver().bind(&fixture.file_root()),
        Err(error) if error.is_not_directory()
    ));

    let backend = fixture.driver().bind(&fixture.root()).unwrap();
    assert!(matches!(
        backend.list(&relative("/a.txt")),
        Err(error) if error.is_not_directory()
    ));
    assert!(matches!(
        backend.read(&relative("/")),
        Err(error) if error.is_directory()
    ));
    assert!(matches!(
        backend.read(&relative("/2026")),
        Err(error) if error.is_directory()
    ));
    assert!(matches!(
        backend.list(&relative("/missing")),
        Err(error) if error.is_not_found()
    ));
    assert!(matches!(
        backend.read(&relative("/missing")),
        Err(error) if error.is_not_found()
    ));
}

/// Generates one integration test per shared driver behavior.
#[macro_export]
macro_rules! driver_conformance_tests {
    ($fixture:ty) => {
        mod conformance {
            use super::*;

            #[test]
            fn listing() {
                let fixture = <$fixture as $crate::Fixture>::new();
                $crate::check_listing(&fixture);
            }

            #[test]
            fn reading() {
                let fixture = <$fixture as $crate::Fixture>::new();
                $crate::check_reading(&fixture);
            }

            #[test]
            fn root_isolation() {
                let fixture = <$fixture as $crate::Fixture>::new();
                $crate::check_root_isolation(&fixture);
            }

            #[test]
            fn errors() {
                let fixture = <$fixture as $crate::Fixture>::new();
                $crate::check_errors(&fixture);
            }
        }
    };
}
