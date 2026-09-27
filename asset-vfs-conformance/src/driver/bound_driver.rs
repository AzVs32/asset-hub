//! Root and capability checks for [`BoundDriver`].

use asset_vfs::driver::{BoundDriver, DriverError};
use asset_vfs::error::VfsError;

use super::data::root_path;
pub use super::data::{BOUND_OTHER_TREE as OTHER_TREE, BOUND_ROOT_TREE as TREE};
use super::support::assert_names;

/// Owns two backends bound to different roots by the same driver.
///
/// Both backends must exist before any check runs. Populate the first root from
/// [`TREE`] and the other from [`OTHER_TREE`]. Keep both roots and their resources
/// alive until the fixture is dropped.
pub trait Fixture: Sized {
    fn new() -> Self;
    fn backend(&self) -> &dyn BoundDriver;
    fn other_backend(&self) -> &dyn BoundDriver;
}

/// Checks that the required reader interprets the empty path as its bound root.
pub fn check_reader_uses_bound_root(fixture: &impl Fixture) {
    let entries = fixture
        .backend()
        .reader()
        .list(&root_path())
        .expect("the bound root must be readable");
    assert_names(&entries, &TREE.listing(&root_path()));
}

/// Checks that interleaved reader access cannot change or leak either root.
pub fn check_bindings_are_independent(fixture: &impl Fixture) {
    let backend = fixture.backend();
    let other_backend = fixture.other_backend();

    for _ in 0..2 {
        assert_names(
            &other_backend.reader().list(&root_path()).unwrap(),
            &OTHER_TREE.listing(&root_path()),
        );
        assert_names(
            &backend.reader().list(&root_path()).unwrap(),
            &TREE.listing(&root_path()),
        );
        for entry in OTHER_TREE.entries() {
            assert!(matches!(
                backend.reader().list(&entry.path()),
                Err(VfsError::Driver(DriverError::NotFound))
            ));
        }
        for entry in TREE.entries() {
            assert!(matches!(
                other_backend.reader().list(&entry.path()),
                Err(VfsError::Driver(DriverError::NotFound))
            ));
        }
    }
}

/// Checks writer exposure against the caller's explicit expectation.
///
/// Neither writer presence nor absence is assumed by the other checks. This
/// checks the capability entry point only, not write operations.
pub fn check_writer_presence(fixture: &impl Fixture, expected: bool) {
    assert_eq!(fixture.backend().as_writer().is_some(), expected);
}
