use super::{Entry, EntryKind, Metadata};
use crate::namespace::domain::EntryName;

#[test]
fn file_entry_may_expose_a_known_size() {
    let entry = Entry::file(EntryName::try_from("a.mp4").unwrap(), Some(42));

    assert_eq!(entry.name().as_str(), "a.mp4");
    assert_eq!(entry.kind(), EntryKind::File);
    assert_eq!(entry.size(), Some(42));
}

#[test]
fn file_entry_may_have_an_unknown_size() {
    let entry = Entry::file(EntryName::try_from("stream").unwrap(), None);

    assert_eq!(entry.kind(), EntryKind::File);
    assert_eq!(entry.size(), None);
}

#[test]
fn directory_entry_has_no_size() {
    let entry = Entry::directory(EntryName::try_from("archive").unwrap());

    assert_eq!(entry.kind(), EntryKind::Directory);
    assert_eq!(entry.size(), None);
}

#[test]
fn metadata_describes_a_path_without_requiring_an_entry_name() {
    let file = Metadata::file(Some(42));
    let directory = Metadata::directory();

    assert_eq!(file.kind(), EntryKind::File);
    assert_eq!(file.size(), Some(42));
    assert_eq!(directory.kind(), EntryKind::Directory);
    assert_eq!(directory.size(), None);
}

#[test]
fn entry_exposes_the_same_metadata_used_by_stat() {
    let entry = Entry::file(EntryName::try_from("a.mp4").unwrap(), Some(42));

    assert_eq!(entry.metadata(), Metadata::file(Some(42)));
}
