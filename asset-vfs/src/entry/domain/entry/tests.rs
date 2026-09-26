use super::{Entry, EntryKind};
use crate::namespace::EntryName;

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
