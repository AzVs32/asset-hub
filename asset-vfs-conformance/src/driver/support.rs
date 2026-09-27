use asset_vfs::entry::{Entry, EntryKind};
use asset_vfs::namespace::{VirtualPath, VirtualRelativePath};

pub(super) fn relative(value: &str) -> VirtualRelativePath {
    VirtualPath::try_from(value)
        .expect("conformance paths must be valid virtual paths")
        .strip_prefix(&VirtualPath::root())
        .unwrap()
}

pub(super) fn assert_names(entries: &[Entry], expected: &[Entry]) {
    let names: Vec<_> = entries.iter().map(|entry| entry.name().as_str()).collect();
    let expected: Vec<_> = expected.iter().map(|entry| entry.name().as_str()).collect();
    assert_eq!(
        names, expected,
        "unexpected direct children or listing order"
    );
}

pub(super) fn assert_metadata(entries: &[Entry], expected: &[Entry]) {
    for expected in expected {
        let name = expected.name().as_str();
        let entry = entries
            .iter()
            .find(|entry| entry.name() == expected.name())
            .unwrap_or_else(|| panic!("missing expected entry {name}"));
        assert_eq!(entry.kind(), expected.kind(), "incorrect kind for {name}");
        match expected.kind() {
            EntryKind::Directory => {
                assert_eq!(entry.size(), None, "directories must not have sizes");
            }
            EntryKind::File => {
                assert!(
                    entry
                        .size()
                        .is_none_or(|size| Some(size) == expected.size()),
                    "{name} has an incorrect byte length: {:?}",
                    entry.size()
                );
            }
        }
    }
}
