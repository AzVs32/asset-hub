use asset_vfs::driver::{DriverError, ReadDriver};
use asset_vfs::entry::{Entry, EntryKind};
use asset_vfs::error::VfsError;
use asset_vfs::namespace::VirtualRelativePath;
use asset_vfs_conformance::driver::data::root_path;
use asset_vfs_conformance::driver::read_driver::{self, Fixture};

// This reader deliberately has no Driver or BoundDriver implementation. Its
// unknown file sizes are valid, while tests can inject contract violations.
struct ReaderFixture {
    root_entries: Vec<Entry>,
}

fn unknown_sizes(entries: Vec<Entry>) -> Vec<Entry> {
    entries
        .into_iter()
        .map(|entry| match entry.kind() {
            EntryKind::File => Entry::file(entry.name().clone(), None),
            EntryKind::Directory => entry,
        })
        .collect()
}

impl Fixture for ReaderFixture {
    fn new() -> Self {
        Self {
            root_entries: unknown_sizes(read_driver::TREE.listing(&root_path())),
        }
    }

    fn reader(&self) -> &dyn ReadDriver {
        self
    }
}

impl ReadDriver for ReaderFixture {
    fn list(&self, path: &VirtualRelativePath) -> Result<Vec<Entry>, VfsError> {
        if path.is_empty() {
            return Ok(self.root_entries.clone());
        }
        match read_driver::TREE
            .entries()
            .iter()
            .find(|entry| entry.path() == *path)
        {
            Some(entry) if entry.kind() == EntryKind::Directory => {
                Ok(unknown_sizes(read_driver::TREE.listing(path)))
            }
            Some(_) => Err(DriverError::NotDirectory.into()),
            None => Err(DriverError::NotFound.into()),
        }
    }
}

#[test]
fn independent_reader_with_unknown_file_sizes_passes_all_read_checks() {
    let fixture = ReaderFixture::new();
    read_driver::check_direct_children(&fixture);
    read_driver::check_listing_order(&fixture);
    read_driver::check_entry_metadata(&fixture);
    read_driver::check_nested_listing(&fixture);
    read_driver::check_empty_directory(&fixture);
    read_driver::check_list_file(&fixture);
    read_driver::check_list_missing(&fixture);
    read_driver::check_list_beneath_file(&fixture);
}

#[test]
#[should_panic(expected = "listing must be strictly ascending")]
fn ordering_check_rejects_disorder_without_affecting_the_name_set_check() {
    let mut fixture = ReaderFixture::new();
    fixture.root_entries.swap(0, 1);
    read_driver::check_direct_children(&fixture);
    read_driver::check_listing_order(&fixture);
}

#[test]
#[should_panic(expected = "root must list each direct child once")]
fn direct_children_check_rejects_duplicate_names() {
    let mut fixture = ReaderFixture::new();
    fixture.root_entries.push(fixture.root_entries[1].clone());
    read_driver::check_direct_children(&fixture);
}

#[test]
#[should_panic(expected = "has an incorrect byte length")]
fn metadata_check_rejects_an_incorrect_known_size() {
    let mut fixture = ReaderFixture::new();
    let expected = read_driver::TREE.listing(&root_path());
    let file = expected
        .iter()
        .find(|entry| entry.kind() == EntryKind::File)
        .unwrap();
    let actual = fixture
        .root_entries
        .iter_mut()
        .find(|entry| entry.name() == file.name())
        .unwrap();
    *actual = Entry::file(file.name().clone(), Some(file.size().unwrap() + 1));
    read_driver::check_entry_metadata(&fixture);
}
