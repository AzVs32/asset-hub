use ah_vfs::domain::storage2::*;
use std::{
    collections::{BTreeSet, HashSet},
    error::Error,
    num::NonZeroUsize,
};
type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

fn entry(path: &str) -> Result<Entry> {
    Ok(Entry::builder()
        .path(StoragePath::parse(path)?)
        .entry_type(EntryType::file().size(0).build())
        .build())
}
fn non_root(path: &str) -> Result<NonRootPath> {
    Ok(StoragePath::parse(path)?.try_into()?)
}

#[test]
fn addresses_preserve_identity_and_all_entry_points_share_validation() -> Result {
    for path in ["", "a", "a/b", "文档/报告", "é", "e\u{301}", "A", "a "] {
        let parsed = StoragePath::parse(path)?;
        assert_eq!(parsed.as_str(), path);
        assert_eq!(path.parse::<StoragePath>()?, parsed);
        assert_eq!(StoragePath::try_from(path.to_owned())?, parsed);
        if let Some(name) = parsed.name() {
            let parent = parsed.parent().ok_or("non-root parent")?;
            assert_eq!(parent.join(&name)?, parsed);
            assert_eq!(name.as_str().parse::<EntryName>()?, name);
            assert_eq!(EntryName::try_from(name.as_str().to_owned())?, name);
        }
    }
    let paths = ["A", "a", "é", "e\u{301}"]
        .map(StoragePath::parse)
        .into_iter()
        .collect::<std::result::Result<Vec<_>, _>>()?;
    assert_eq!(paths.iter().cloned().collect::<HashSet<_>>().len(), 4);
    assert_eq!(paths.into_iter().collect::<BTreeSet<_>>().len(), 4);
    assert!(StoragePath::root().parent().is_none());
    assert!(StoragePath::root().name().is_none());
    Ok(())
}
#[test]
fn invalid_addresses_cannot_enter_through_owned_or_parsed_conversions() {
    for path in [
        "/absolute",
        "a//b",
        "a/",
        ".",
        "a/..",
        "a\\b",
        "a:b",
        "a\0b",
    ] {
        let error = StoragePath::parse(path).err();
        assert!(error.is_some());
        assert_eq!(path.parse::<StoragePath>().err(), error);
        assert_eq!(StoragePath::try_from(path.to_owned()).err(), error);
    }
    for name in ["", ".", "..", "a/b", "a\\b", "a:b", "\0"] {
        let error = EntryName::parse(name).err();
        assert!(error.is_some());
        assert_eq!(name.parse::<EntryName>().err(), error);
        assert_eq!(EntryName::try_from(name.to_owned()).err(), error);
    }
}
#[test]
fn address_bounds_count_utf8_bytes_and_join_rechecks_the_total() -> Result {
    assert!(StoragePath::parse(&"x".repeat(MAX_PATH_BYTES)).is_ok());
    assert_eq!(
        StoragePath::parse(&"x".repeat(MAX_PATH_BYTES + 1)),
        Err(PathError::TooLong)
    );
    assert_eq!(
        StoragePath::parse(&"文".repeat(MAX_PATH_BYTES / 3 + 1)),
        Err(PathError::TooLong)
    );
    let parent = StoragePath::parse(&"p".repeat(MAX_PATH_BYTES - 2))?;
    assert_eq!(
        parent.join(&EntryName::parse("a")?)?.as_str().len(),
        MAX_PATH_BYTES
    );
    assert_eq!(
        parent.join(&EntryName::parse("ab")?),
        Err(PathError::TooLong)
    );
    assert_eq!(
        ListRequest::builder()
            .directory(parent)
            .after(EntryName::parse("ab")?)
            .build(),
        Err(PathError::TooLong)
    );
    Ok(())
}
#[test]
fn root_protection_is_checked_before_constructing_a_mutating_request() -> Result {
    assert!(NonRootPath::try_from(StoragePath::root()).is_err());
    let request = WriteRequest::builder()
        .path(non_root("file")?)
        .mode(WriteMode::CreateNew)
        .publication(Publication::Atomic)
        .build();
    assert_eq!(request.path().as_path().as_str(), "file");
    let rename = RenameRequest::builder()
        .source(non_root("a")?)
        .destination(non_root("b")?)
        .mode(RenameMode::NoReplace)
        .build();
    assert_eq!(rename.source().as_path().as_str(), "a");
    assert_eq!(rename.destination().as_path().as_str(), "b");
    let recursive = CreateDirectoryRequest::builder()
        .path(StoragePath::root())
        .mode(CreateDirectoryMode::Recursive)
        .build();
    assert!(recursive.path().is_root());
    Ok(())
}
#[test]
fn ranges_handle_empty_reads_eof_and_integer_boundaries() -> Result {
    assert_eq!(ReadRange::default(), ReadRange::FULL);
    assert_eq!(
        ReadRange::builder()
            .offset(u64::MAX)
            .maybe_length(None)
            .build()?
            .end_exclusive(),
        None
    );
    assert_eq!(
        ReadRange::builder()
            .offset(u64::MAX)
            .maybe_length(Some(0))
            .build()?
            .end_exclusive(),
        Some(u64::MAX)
    );
    assert_eq!(
        ReadRange::builder()
            .offset(u64::MAX - 2)
            .maybe_length(Some(2))
            .build()?
            .end_exclusive(),
        Some(u64::MAX)
    );
    assert_eq!(
        ReadRange::builder()
            .offset(u64::MAX)
            .maybe_length(Some(1))
            .build(),
        Err(RangeError::Overflow)
    );
    assert_eq!(
        ReadRange::builder()
            .offset(1)
            .maybe_length(Some(u64::MAX))
            .build(),
        Err(RangeError::Overflow)
    );
    Ok(())
}
#[test]
fn metadata_carries_file_length_only_in_the_file_variant() -> Result {
    let time = std::time::UNIX_EPOCH;
    let file = Entry::builder()
        .path(StoragePath::parse("file")?)
        .entry_type(EntryType::file().size(0).build())
        .modified(time)
        .build();
    assert_eq!(file.entry_type(), EntryType::file().size(0).build());
    assert_eq!(file.modified(), Some(time));
    for kind in [
        EntryType::Directory,
        EntryType::Symlink,
        EntryType::Other,
        EntryType::file().build(),
    ] {
        let entry = Entry::builder()
            .path(StoragePath::parse("entry")?)
            .entry_type(kind)
            .build();
        assert_eq!(entry.entry_type(), kind);
        assert!(entry.modified().is_none());
    }
    Ok(())
}
#[test]
fn permissions_and_attribute_names_validate_their_exact_portable_domains() -> Result {
    for bits in 0..=0o7777 {
        assert_eq!(Permissions::new(bits)?.bits(), bits);
    }
    for bits in [0o10000, 0o100644, u32::MAX] {
        assert_eq!(Permissions::new(bits), Err(PermissionsError::InvalidBits));
    }
    for name in [
        "x".repeat(255),
        "文".repeat(85),
        "user.é".into(),
        "user.e\u{301}".into(),
    ] {
        assert_eq!(XattrName::parse(&name)?.as_str(), name);
    }
    assert_eq!(XattrName::parse(""), Err(XattrNameError::Empty));
    assert_eq!(XattrName::parse("user.\0x"), Err(XattrNameError::Nul));
    assert_eq!(
        XattrName::parse(&"x".repeat(256)),
        Err(XattrNameError::TooLong)
    );
    assert_eq!(
        XattrName::parse(&"文".repeat(86)),
        Err(XattrNameError::TooLong)
    );
    assert_ne!(
        XattrName::parse("user.é")?,
        XattrName::parse("user.e\u{301}")?
    );
    Ok(())
}
#[test]
fn attribute_lists_are_complete_ordered_and_bounded_in_encoded_bytes() -> Result {
    let names =
        || -> Result<Vec<XattrName>> { Ok(vec![XattrName::parse("a")?, XattrName::parse("文")?]) };
    let list = XattrList::builder()
        .names(names()?)
        .max_bytes(NonZeroUsize::new(6).ok_or("limit")?)
        .build()?;
    assert_eq!(list.names().len(), 2);
    assert_eq!(
        XattrList::builder()
            .names(names()?)
            .max_bytes(NonZeroUsize::new(5).ok_or("limit")?)
            .build(),
        Err(XattrListError::TooLarge)
    );
    assert_eq!(
        XattrList::builder()
            .names(vec![XattrName::parse("a")?, XattrName::parse("a")?])
            .max_bytes(NonZeroUsize::MIN)
            .build(),
        Err(XattrListError::NotOrdered)
    );
    assert_eq!(
        XattrList::builder()
            .names(vec![XattrName::parse("b")?, XattrName::parse("a")?])
            .max_bytes(NonZeroUsize::MIN)
            .build(),
        Err(XattrListError::NotOrdered)
    );
    assert!(
        XattrList::builder()
            .names(vec![])
            .max_bytes(NonZeroUsize::MIN)
            .build()?
            .names()
            .is_empty()
    );
    Ok(())
}
#[test]
fn page_size_cannot_represent_zero_or_an_oversized_page() -> Result {
    assert_eq!(PageSize::new(0), Err(PageSizeError::Zero));
    assert_eq!(
        PageSize::new(PageSize::MAX + 1),
        Err(PageSizeError::TooLarge)
    );
    assert_eq!(PageSize::new(usize::MAX), Err(PageSizeError::TooLarge));
    assert_eq!(PageSize::new(PageSize::MAX)?.get(), PageSize::MAX);
    assert_eq!(PageSize::default().get(), 100);
    Ok(())
}
#[test]
fn directory_pages_check_scope_order_cursor_limit_and_forward_progress() -> Result {
    let request = ListRequest::builder()
        .directory(StoragePath::parse("dir")?)
        .size(PageSize::new(2)?)
        .build()?;
    let page = DirectoryPage::builder()
        .request(&request)
        .entries(vec![entry("dir/a")?, entry("dir/b")?])
        .has_more(true)
        .build()?;
    assert_eq!(page.next_after().ok_or("cursor")?.as_str(), "b");
    let next = ListRequest::builder()
        .directory(request.directory().clone())
        .size(request.size())
        .after(page.next_after().ok_or("cursor")?.clone())
        .build()?;
    let last = DirectoryPage::builder()
        .request(&next)
        .entries(vec![entry("dir/c")?])
        .has_more(false)
        .build()?;
    assert!(last.next_after().is_none());
    for (entries, more, expected) in [
        (
            vec![entry("dir/a")?, entry("dir/b")?, entry("dir/c")?],
            false,
            DirectoryPageError::TooManyEntries,
        ),
        (
            vec![entry("elsewhere/a")?],
            false,
            DirectoryPageError::NotAChild,
        ),
        (
            vec![entry("dir/nested/a")?],
            false,
            DirectoryPageError::NotAChild,
        ),
        (vec![entry("dir")?], false, DirectoryPageError::NotAChild),
        (
            vec![entry("dir/b")?, entry("dir/a")?],
            false,
            DirectoryPageError::NotOrdered,
        ),
        (
            vec![entry("dir/a")?, entry("dir/a")?],
            false,
            DirectoryPageError::NotOrdered,
        ),
        (vec![], true, DirectoryPageError::EmptyContinuation),
    ] {
        assert_eq!(
            DirectoryPage::builder()
                .request(&request)
                .entries(entries)
                .has_more(more)
                .build(),
            Err(expected)
        );
    }
    assert_eq!(
        DirectoryPage::builder()
            .request(&next)
            .entries(vec![entry("dir/b")?])
            .has_more(false)
            .build(),
        Err(DirectoryPageError::BeforeCursor)
    );
    assert!(
        DirectoryPage::builder()
            .request(&request)
            .entries(vec![])
            .has_more(false)
            .build()?
            .entries()
            .is_empty()
    );
    let root = ListRequest::builder()
        .directory(StoragePath::root())
        .build()?;
    assert!(
        DirectoryPage::builder()
            .request(&root)
            .entries(vec![entry("a")?])
            .has_more(false)
            .build()
            .is_ok()
    );
    assert_eq!(
        DirectoryPage::builder()
            .request(&root)
            .entries(vec![
                Entry::builder()
                    .path(StoragePath::root())
                    .entry_type(EntryType::Directory)
                    .build()
            ])
            .has_more(false)
            .build(),
        Err(DirectoryPageError::NotAChild)
    );
    Ok(())
}

#[test]
fn response_decoding_checks_the_supplied_continuation_instead_of_replacing_it() -> Result {
    let request = ListRequest::builder()
        .directory(StoragePath::parse("dir")?)
        .build()?;
    let page = DirectoryPage::from_parts()
        .request(&request)
        .entries(vec![entry("dir/a")?])
        .next(Some(EntryName::parse("a")?))
        .build()?;
    let (entries, cursor) = page.into_parts();
    assert_eq!(entries.len(), 1);
    assert_eq!(cursor.ok_or("continuation")?.as_str(), "a");
    assert_eq!(
        DirectoryPage::from_parts()
            .request(&request)
            .entries(vec![entry("dir/a")?])
            .next(Some(EntryName::parse("b")?))
            .build(),
        Err(DirectoryPageError::MismatchedCursor)
    );
    assert_eq!(
        DirectoryPage::from_parts()
            .request(&request)
            .entries(vec![])
            .next(Some(EntryName::parse("a")?))
            .build(),
        Err(DirectoryPageError::EmptyContinuation)
    );
    assert!(
        DirectoryPage::from_parts()
            .request(&request)
            .entries(vec![entry("dir/a")?])
            .next(None)
            .build()?
            .next_after()
            .is_none()
    );
    Ok(())
}

#[test]
fn a_full_page_requires_an_explicit_choice_between_final_and_continuing() -> Result {
    let request = ListRequest::builder()
        .directory(StoragePath::root())
        .size(PageSize::new(1)?)
        .build()?;
    for has_more in [false, true] {
        let page = DirectoryPage::builder()
            .request(&request)
            .entries(vec![entry("a")?])
            .has_more(has_more)
            .build()?;
        assert_eq!(page.next_after().is_some(), has_more);
        let (entries, next) = page.into_parts();
        let decoded = DirectoryPage::from_parts()
            .request(&request)
            .entries(entries)
            .next(next)
            .build()?;
        assert_eq!(
            decoded.next_after().map(|name| name.as_str()),
            if has_more { Some("a") } else { None }
        );
    }
    let empty = DirectoryPage::from_parts()
        .request(&request)
        .entries(vec![])
        .next(None)
        .build()?;
    assert!(empty.entries().is_empty());
    assert!(empty.next_after().is_none());
    Ok(())
}
