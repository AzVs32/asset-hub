use ah_vfs_storage::*;
use std::{
    collections::{BTreeSet, HashSet},
    error::Error,
    num::NonZeroUsize,
};
type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

fn entry(path: &str) -> Result<Entry> {
    Ok(Entry::new(
        StoragePath::parse(path)?,
        EntryType::file(Some(0)),
        None,
    )?)
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
    Ok(())
}

#[test]
fn authoritative_backend_names_are_decoded_exactly_or_rejected() -> Result {
    for text in ["Report", "report", "é", "e\u{301}", "trailing "] {
        assert_eq!(
            EntryName::from_backend_bytes(text.as_bytes())?.as_str(),
            text
        );
        assert_eq!(
            XattrName::from_backend_bytes(text.as_bytes())?.as_str(),
            text
        );
    }
    for bytes in [
        b"".as_slice(),
        b"\xff",
        b".",
        b"..",
        b"a/b",
        b"a\\b",
        b"a:b",
        b"a\0b",
    ] {
        assert_eq!(
            EntryName::from_backend_bytes(bytes),
            Err(BackendFailure::UnrepresentableName)
        );
    }
    for bytes in [b"".as_slice(), b"\xff", b"a\0b"] {
        assert_eq!(
            XattrName::from_backend_bytes(bytes),
            Err(BackendFailure::UnrepresentableName)
        );
    }
    assert_eq!(XattrName::from_backend_bytes(b"a/b:c")?.as_str(), "a/b:c");

    let maximum = "a".repeat(MAX_PATH_BYTES);
    assert!(EntryName::from_backend_bytes(maximum.as_bytes()).is_ok());
    let too_long = format!("{maximum}a");
    assert_eq!(EntryName::parse(&too_long), Err(NameError::TooLong));
    assert_eq!(
        EntryName::from_backend_bytes(too_long.as_bytes()),
        Err(BackendFailure::UnrepresentableName)
    );
    let max_key = "文".repeat(XattrName::MAX_BYTES / '文'.len_utf8());
    assert!(XattrName::from_backend_bytes(max_key.as_bytes()).is_ok());
    let too_long = format!("{max_key}a");
    assert_eq!(XattrName::parse(&too_long), Err(XattrNameError::TooLong));
    assert_eq!(
        XattrName::from_backend_bytes(too_long.as_bytes()),
        Err(BackendFailure::UnrepresentableName)
    );
    Ok(())
}
#[test]
fn canonical_mutation_requests_reject_root() -> Result {
    let root = StoragePath::root();
    let file = StoragePath::parse("file")?;
    assert_eq!(
        WriteRequest::new(root.clone(), WriteMode::CreateNew, Publication::Atomic),
        Err(RequestPathError::RootNotAllowed)
    );
    assert_eq!(
        RemoveRequest::new(root.clone(), RemoveKind::NonDirectory),
        Err(RequestPathError::RootNotAllowed)
    );
    assert_eq!(
        RenameRequest::new(file.clone(), root.clone(), RenameMode::Replace),
        Err(RequestPathError::RootNotAllowed)
    );
    assert_eq!(
        RenameRequest::new(root.clone(), file, RenameMode::NoReplace),
        Err(RequestPathError::RootNotAllowed)
    );
    assert_eq!(
        CreateDirectoryRequest::new(root.clone()),
        Err(RequestPathError::RootNotAllowed)
    );
    assert_eq!(StatRequest::new(root.clone()).path(), &root);
    assert_eq!(
        ReadRequest::new(root.clone(), ReadRange::FULL).path(),
        &root
    );
    Ok(())
}

#[test]
fn ranges_handle_empty_reads_eof_and_integer_boundaries() -> Result {
    assert_eq!(ReadRange::default(), ReadRange::FULL);
    assert_eq!(ReadRange::new(u64::MAX, None)?.length(), None);
    assert_eq!(ReadRange::new(u64::MAX, Some(0))?.length(), Some(0));
    assert_eq!(ReadRange::new(u64::MAX - 2, Some(2))?.length(), Some(2));
    assert_eq!(ReadRange::new(u64::MAX, Some(1)), Err(RangeError::Overflow));
    assert_eq!(ReadRange::new(1, Some(u64::MAX)), Err(RangeError::Overflow));
    Ok(())
}
#[test]
fn metadata_carries_file_length_only_in_the_file_variant() -> Result {
    let time = std::time::UNIX_EPOCH;
    let file = Entry::new(
        StoragePath::parse("file")?,
        EntryType::file(Some(0)),
        Some(time),
    )?;
    assert_eq!(file.entry_type(), EntryType::file(Some(0)));
    assert_eq!(file.modified(), Some(time));
    for kind in [
        EntryType::Directory,
        EntryType::Symlink,
        EntryType::Other,
        EntryType::file(None),
    ] {
        let entry = Entry::new(StoragePath::parse("entry")?, kind, None)?;
        assert_eq!(entry.entry_type(), kind);
        assert!(entry.modified().is_none());
    }
    for kind in [EntryType::file(None), EntryType::Symlink, EntryType::Other] {
        assert_eq!(
            Entry::new(StoragePath::root(), kind, None),
            Err(EntryError::RootNotDirectory)
        );
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
    let list = XattrList::new(names()?, NonZeroUsize::new(6).ok_or("limit")?)?;
    assert_eq!(list.names().len(), 2);
    assert_eq!(
        XattrList::new(names()?, NonZeroUsize::new(5).ok_or("limit")?),
        Err(XattrListError::TooLarge)
    );
    assert_eq!(
        XattrList::new(
            vec![XattrName::parse("a")?, XattrName::parse("a")?],
            NonZeroUsize::MIN
        ),
        Err(XattrListError::NotOrdered)
    );
    assert_eq!(
        XattrList::new(
            vec![XattrName::parse("b")?, XattrName::parse("a")?],
            NonZeroUsize::MIN
        ),
        Err(XattrListError::NotOrdered)
    );
    assert!(
        XattrList::new(vec![], NonZeroUsize::MIN)?
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
fn selected(address: &str, identity: u8) -> Result<DirectorySelection> {
    Ok(DirectorySelection::from_lookup(
        StoragePath::parse(address)?,
        DirectoryIdentity::from_bytes([identity; 16]),
    ))
}

#[test]
fn directory_pages_check_scope_order_cursor_limit_and_forward_progress() -> Result {
    let request = ListRequest::new(StoragePath::parse("dir")?, PageSize::new(2)?);
    let bound = request.bind_selection(selected("dir", 1)?)?;
    let page = DirectoryPage::new(&bound, vec![entry("dir/a")?, entry("dir/b")?], true)?;
    let cursor = page.continuation().ok_or("continuation")?;
    assert_eq!(cursor.after().as_str(), "b");
    assert_eq!(cursor.selection(), bound.selection());
    let next = ListRequest::continuing(cursor, request.size());
    let resumed = next.bind_selection(bound.selection().clone())?;
    let last = DirectoryPage::new(&resumed, vec![entry("dir/c")?], false)?;
    assert!(last.continuation().is_none());

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
        assert_eq!(DirectoryPage::new(&bound, entries, more), Err(expected));
    }
    assert_eq!(
        DirectoryPage::new(&resumed, vec![entry("dir/b")?], false),
        Err(DirectoryPageError::BeforeCursor)
    );
    assert!(
        DirectoryPage::new(&resumed, vec![], false)?
            .entries()
            .is_empty()
    );

    let root = ListRequest::new(StoragePath::root(), PageSize::DEFAULT);
    let bound_root = root.bind_selection(selected("", 2)?)?;
    assert!(DirectoryPage::new(&bound_root, vec![entry("a")?], false).is_ok());
    let root_entry = Entry::new(StoragePath::root(), EntryType::Directory, None)?;
    assert_eq!(
        DirectoryPage::new(&bound_root, vec![root_entry], false),
        Err(DirectoryPageError::NotAChild)
    );
    Ok(())
}

#[test]
fn decoder_checks_directory_claims_against_lookup_even_for_empty_pages() -> Result {
    let request = ListRequest::new(StoragePath::parse("dir")?, PageSize::DEFAULT);
    let bound = request.bind_selection(selected("dir", 1)?)?;
    let actual = bound.selection();
    let correct = DirectoryPage::from_parts(
        &bound,
        actual.address(),
        actual.identity(),
        vec![entry("dir/a")?],
        None,
    )?;
    assert_eq!(correct.selection(), actual);

    let wrong = StoragePath::parse("elsewhere")?;
    for (reported_address, reported_identity, entries) in [
        (
            &wrong,
            DirectoryIdentity::from_bytes([2; 16]),
            vec![entry("elsewhere/a")?],
        ),
        (&wrong, actual.identity(), vec![]),
        (
            actual.address(),
            DirectoryIdentity::from_bytes([2; 16]),
            vec![],
        ),
    ] {
        assert_eq!(
            DirectoryPage::from_parts(&bound, reported_address, reported_identity, entries, None),
            Err(DirectoryPageError::UnexpectedDirectory)
        );
    }
    assert_eq!(
        DirectoryPage::from_parts(
            &bound,
            actual.address(),
            actual.identity(),
            vec![entry("elsewhere/a")?],
            None
        ),
        Err(DirectoryPageError::NotAChild)
    );
    Ok(())
}

#[test]
fn continuation_requires_the_same_directory_address_and_identity() -> Result {
    for (first_address, changed_address) in [("dir", "retargeted"), ("work-a/dir", "work-b/dir")] {
        let request = ListRequest::new(StoragePath::parse(first_address)?, PageSize::DEFAULT);
        assert!(request.after().is_none());
        let original = selected(first_address, 1)?;
        let bound = request.bind_selection(original.clone())?;
        let first = DirectoryPage::new(&bound, vec![entry(&format!("{first_address}/a"))?], true)?;
        let request = ListRequest::continuing(
            first.continuation().ok_or("continuation")?,
            PageSize::DEFAULT,
        );
        assert!(request.after().is_some());

        for wrong in [
            selected(changed_address, 2)?,
            selected(first_address, 3)?,
            selected("moved", 1)?,
        ] {
            assert!(matches!(
                request.bind_selection(wrong),
                Err(BackendFailure::Stale)
            ));
        }
        let resumed = request.bind_selection(original.clone())?;
        let second =
            DirectoryPage::new(&resumed, vec![entry(&format!("{first_address}/c"))?], false)?;
        assert_eq!(second.selection(), &original);
        assert!(second.continuation().is_none());
    }
    Ok(())
}

#[test]
fn response_decoding_checks_the_supplied_continuation_instead_of_replacing_it() -> Result {
    let request = ListRequest::new(StoragePath::parse("dir")?, PageSize::DEFAULT);
    let bound = request.bind_selection(selected("dir", 1)?)?;
    let actual = bound.selection();
    let decode = |entries, next| {
        DirectoryPage::from_parts(&bound, actual.address(), actual.identity(), entries, next)
    };
    let page = decode(vec![entry("dir/a")?], Some(EntryName::parse("a")?))?;
    let (directory, entries, cursor) = page.into_parts();
    assert_eq!(directory, *actual);
    assert_eq!(entries.len(), 1);
    assert_eq!(cursor.ok_or("continuation")?.as_str(), "a");
    assert_eq!(
        decode(vec![entry("dir/a")?], Some(EntryName::parse("b")?)),
        Err(DirectoryPageError::MismatchedCursor)
    );
    assert_eq!(
        decode(vec![], Some(EntryName::parse("a")?)),
        Err(DirectoryPageError::EmptyContinuation)
    );
    assert!(
        decode(vec![entry("dir/a")?], None)?
            .continuation()
            .is_none()
    );
    Ok(())
}

#[test]
fn a_full_page_requires_an_explicit_choice_between_final_and_continuing() -> Result {
    let request = ListRequest::new(StoragePath::root(), PageSize::new(1)?);
    let bound = request.bind_selection(selected("", 1)?)?;
    for has_more in [false, true] {
        let page = DirectoryPage::new(&bound, vec![entry("a")?], has_more)?;
        assert_eq!(page.continuation().is_some(), has_more);
        let (directory, entries, next) = page.into_parts();
        let decoded = DirectoryPage::from_parts(
            &bound,
            directory.address(),
            directory.identity(),
            entries,
            next,
        )?;
        assert_eq!(
            decoded
                .continuation()
                .map(|cursor| cursor.after().to_string()),
            has_more.then(|| "a".into())
        );
    }
    let empty = DirectoryPage::from_parts(
        &bound,
        bound.selection().address(),
        bound.selection().identity(),
        vec![],
        None,
    )?;
    assert!(empty.entries().is_empty());
    assert!(empty.continuation().is_none());
    Ok(())
}
