//! Directory-listing checks for [`ReadDriver`].
//!
//! Fixtures expose [`TREE`]. Populate it using a native
//! [`TreeBuilder`](super::data::TreeBuilder), or provide matching pre-provisioned
//! storage. Names, paths, kinds, and byte lengths are derived from the shared
//! tree; changing its entries does not require rewriting native fixtures.

use asset_vfs::driver::{DriverError, ReadDriver};
use asset_vfs::entry::EntryKind;
use asset_vfs::error::VfsError;

pub use super::data::READ_TREE as TREE;
use super::data::{missing_child, root_path};
use super::support::{assert_metadata, assert_names};

/// Owns a reader exposing [`TREE`] and all resources it needs.
///
/// No driver factory, bound-backend accessor, or production writer is required.
pub trait Fixture: Sized {
    fn new() -> Self;
    fn reader(&self) -> &dyn ReadDriver;
}

/// Checks the exact direct-child name set, without requiring ordering here.
pub fn check_direct_children(fixture: &impl Fixture) {
    let entries = fixture.reader().list(&root_path()).unwrap();
    let expected = TREE.listing(&root_path());
    let mut names: Vec<_> = entries.iter().map(|entry| entry.name().as_str()).collect();
    names.sort_unstable();
    let expected: Vec<_> = expected.iter().map(|entry| entry.name().as_str()).collect();
    assert_eq!(names, expected, "root must list each direct child once");
}

/// Checks ascending name order without duplicates.
pub fn check_listing_order(fixture: &impl Fixture) {
    let entries = fixture.reader().list(&root_path()).unwrap();
    assert!(
        entries
            .windows(2)
            .all(|pair| pair[0].name() < pair[1].name()),
        "listing must be strictly ascending by EntryName"
    );
}

/// Checks root entry kinds and optional byte lengths derived from [`TREE`].
pub fn check_entry_metadata(fixture: &impl Fixture) {
    let entries = fixture.reader().list(&root_path()).unwrap();
    assert_metadata(&entries, &TREE.listing(&root_path()));
}

/// Checks every described subdirectory, including Unicode and deeper paths.
pub fn check_nested_listing(fixture: &impl Fixture) {
    let reader = fixture.reader();
    for directory in TREE
        .entries()
        .iter()
        .filter(|entry| entry.kind() == EntryKind::Directory)
    {
        let path = directory.path();
        let entries = reader.list(&path).unwrap();
        let expected = TREE.listing(&path);
        assert_names(&entries, &expected);
        assert_metadata(&entries, &expected);
    }
}

/// Checks that the empty directories described by [`TREE`] list successfully.
pub fn check_empty_directory(fixture: &impl Fixture) {
    let mut checked = false;
    for directory in TREE
        .entries()
        .iter()
        .filter(|entry| entry.kind() == EntryKind::Directory)
    {
        let path = directory.path();
        if TREE.listing(&path).is_empty() {
            assert!(fixture.reader().list(&path).unwrap().is_empty());
            checked = true;
        }
    }
    assert!(checked, "the read tree must contain an empty directory");
}

/// Checks that listing any described file reports `NotDirectory`.
pub fn check_list_file(fixture: &impl Fixture) {
    for file in TREE
        .entries()
        .iter()
        .filter(|entry| entry.kind() == EntryKind::File)
    {
        assert!(matches!(
            fixture.reader().list(&file.path()),
            Err(VfsError::Driver(DriverError::NotDirectory))
        ));
    }
}

/// Checks absent children beneath the root and every described directory.
pub fn check_list_missing(fixture: &impl Fixture) {
    let directories = std::iter::once(root_path()).chain(
        TREE.entries()
            .iter()
            .filter(|entry| entry.kind() == EntryKind::Directory)
            .map(|entry| entry.path()),
    );
    for directory in directories {
        assert!(matches!(
            fixture.reader().list(&missing_child(&directory)),
            Err(VfsError::Driver(DriverError::NotFound))
        ));
    }
}

/// Checks file ancestors, accepting either error allowed by the contract.
pub fn check_list_beneath_file(fixture: &impl Fixture) {
    for file in TREE
        .entries()
        .iter()
        .filter(|entry| entry.kind() == EntryKind::File)
    {
        assert!(matches!(
            fixture.reader().list(&missing_child(&file.path())),
            Err(VfsError::Driver(
                DriverError::NotFound | DriverError::NotDirectory
            ))
        ));
    }
}
