//! Storage behavior shared by independent driver test suites.

use crate::application::port::{
    EntrySlot, ObservedEntry, OperationLookup, Storage, StorageFileReader,
};
use crate::resolve::StorageOperations;
use crate::resolve::operand::{CreateDirectoryOperand, RenameOperand};
use crate::resolve::operand::{ListOperand, ListOperandTarget};
use crate::resolve::path::{PathExpr, RootedPath};
use ah_vfs_storage::RenameMode;
use ah_vfs_storage::RenameSupport;
use ah_vfs_storage::{
    BackendFailure, Capabilities, EntryName, EntryType, MutationOutcome, Operation,
};
use ah_vfs_storage::{Failure, Rejection, StorageError, StoragePath};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

/// A Stat lookup accepts its own root observation and rejects another
/// lookup's observation, even when both address the same root.
pub async fn assert_stat_binding_contract(storage: &dyn Storage) -> TestResult {
    use ah_vfs_storage::StatRequest;

    let mut other = storage.begin_lookup(Operation::Stat).await?;
    let mut current = storage.begin_lookup(Operation::Stat).await?;
    let foreign = other.root().await?;
    let own = current.root().await?;
    let request = StatRequest::new(StoragePath::root());
    let error = current
        .execute_stat(None, &foreign, request.clone())
        .await
        .err()
        .ok_or("foreign stat observation accepted")?;
    assert_eq!(error.failure(), Failure::Backend(BackendFailure::Stale));
    assert_eq!(error.outcome(), MutationOutcome::Unchanged);
    assert_eq!(
        current.execute_stat(None, &own, request).await?,
        *own.entry()
    );
    Ok(())
}

/// Check scope, address and live-slot validation for each advertised operation.
/// Use an isolated fixture with a `binding-dir` containing only `link`, a
/// symlink to `file`, if ReadLink is advertised. The suite creates other
/// entries and replaces their parent using no-replace rename and mkdir.
pub async fn assert_execution_binding_contract(
    storage: &dyn Storage,
    capabilities: Capabilities,
) -> TestResult {
    let parent_name = EntryName::parse("binding-dir")?;
    let destination = EntryName::parse("moved")?;
    let wrong = path("binding-dir/wrong")?;
    mkdir(storage, "binding-dir/subdir").await?;
    put(storage, "binding-file", b"original").await?;
    storage
        .move_entry(
            RenameOperand::new(
                PathExpr::parse("/binding-file")?,
                PathExpr::parse("/binding-dir/file")?,
                RenameMode::NoReplace,
            )?,
            Default::default(),
        )
        .await?;

    let mut observed = Vec::new();
    for op in [
        Operation::Stat,
        Operation::List,
        Operation::Read,
        Operation::ReadLink,
        Operation::Write,
        Operation::CreateDirectory,
        Operation::Remove,
        Operation::Rename,
        Operation::GetPermissions,
        Operation::SetPermissions,
        Operation::ListXattrs,
        Operation::GetXattr,
        Operation::SetXattr,
        Operation::RemoveXattr,
    ]
    .into_iter()
    .filter(|op| capabilities.supports(*op))
    {
        let name = EntryName::parse(match op {
            Operation::List => "subdir",
            Operation::ReadLink => "link",
            Operation::Write | Operation::CreateDirectory => "new",
            _ => "file",
        })?;
        let (mut lookup, parent, entry) =
            observe_slot(storage, op, &parent_name, &name, &destination).await?;
        let (_foreign_lookup, foreign_parent, foreign_entry) =
            observe_slot(storage, op, &parent_name, &name, &destination).await?;
        let address = parent.entry().path().join(&name)?;
        let foreign = EntrySlot {
            parent: &foreign_parent,
            name: &name,
            entry: foreign_entry.as_ref(),
        };
        let local = EntrySlot {
            parent: &parent,
            name: &name,
            entry: entry.as_ref(),
        };
        assert_execution_failure(
            op,
            "foreign scope",
            execute_slot(lookup.as_mut(), foreign, &destination, address).await?,
            Failure::Backend(BackendFailure::Stale),
        )?;
        assert_execution_failure(
            op,
            "wrong request address",
            execute_slot(lookup.as_mut(), local, &destination, wrong.clone()).await?,
            Failure::InvalidBinding,
        )?;
        observed.push((lookup, parent, name, entry));
    }

    // Replace the parent once after all the lookups have bound it. This also
    // covers operations whose final slot was observed absent.
    storage
        .move_entry(
            RenameOperand::new(
                PathExpr::parse("/binding-dir")?,
                PathExpr::parse("/retired-dir")?,
                RenameMode::NoReplace,
            )?,
            Default::default(),
        )
        .await?;
    mkdir(storage, "binding-dir").await?;
    // The write case uses a root slot so it also works on backends that
    // restrict publication to the root. Replace its observed absence.
    if capabilities.supports(Operation::Write) {
        put(storage, "new", b"occupant").await?;
    }

    for (mut lookup, parent, name, entry) in observed {
        let op = lookup.operation();
        let address = parent.entry().path().join(&name)?;
        let slot = EntrySlot {
            parent: &parent,
            name: &name,
            entry: entry.as_ref(),
        };
        assert_execution_failure(
            op,
            "changed namespace",
            execute_slot(lookup.as_mut(), slot, &destination, address).await?,
            Failure::Rejected(Rejection::Conflict),
        )?;
    }
    assert_eq!(get(storage, "retired-dir/file").await?, b"original");
    assert!(!exists(storage, "retired-dir/new").await?);
    assert!(!exists(storage, "binding-dir/new").await?);
    assert!(!exists(storage, "binding-dir/moved").await?);
    if capabilities.supports(Operation::Write) {
        assert_eq!(get(storage, "new").await?, b"occupant");
    }
    Ok(())
}

async fn observe_slot(
    storage: &dyn Storage,
    op: Operation,
    parent_name: &EntryName,
    name: &EntryName,
    destination: &EntryName,
) -> TestResult<(
    Box<dyn OperationLookup>,
    ObservedEntry,
    Option<ObservedEntry>,
)> {
    let mut lookup = storage.begin_lookup(op).await?;
    let root = lookup.root().await?;
    let parent = if op == Operation::Write {
        root
    } else {
        lookup.observe(&root, parent_name).await?
    };
    let entry = match lookup.observe(&parent, name).await {
        Ok(entry) => Some(entry),
        Err(error)
            if matches!(op, Operation::Write | Operation::CreateDirectory)
                && error.failure() == Failure::Rejected(Rejection::NotFound) =>
        {
            None
        }
        Err(error) => return Err(error.into()),
    };
    if op == Operation::Rename {
        assert!(matches!(lookup.observe(&parent, destination).await,
            Err(error) if error.failure() == Failure::Rejected(Rejection::NotFound)));
    }
    Ok((lookup, parent, entry))
}

async fn execute_slot(
    lookup: &mut dyn OperationLookup,
    slot: EntrySlot<'_>,
    destination: &EntryName,
    address: StoragePath,
) -> TestResult<Result<(), StorageError>> {
    use ah_vfs_storage::{
        AttributeAction as A, AttributeRequest, CreateDirectoryRequest, ListRequest, PageSize,
        Permissions, Publication, ReadLinkRequest, ReadRange, ReadRequest, RemoveKind,
        RemoveRequest, RenameRequest, StatRequest, WriteMode, WriteRequest, XattrName,
        XattrWriteMode,
    };
    use std::num::NonZeroUsize;

    let entry = || {
        slot.entry
            .ok_or("expected an observed target in the fixture")
    };
    let result = match lookup.operation() {
        Operation::Stat => lookup
            .execute_stat(Some(slot), entry()?, StatRequest::new(address))
            .await
            .map(drop),
        Operation::List => lookup
            .select_directory(
                Some(slot),
                entry()?,
                ListRequest::new(address, PageSize::DEFAULT),
            )
            .await
            .map(drop),
        Operation::Read => lookup
            .execute_read(slot, ReadRequest::new(address, ReadRange::FULL))
            .await
            .map(drop),
        Operation::ReadLink => lookup
            .execute_read_link(
                slot.parent,
                slot.name,
                entry()?,
                ReadLinkRequest::new(address),
            )
            .await
            .map(drop),
        Operation::Write => lookup
            .execute_write(
                slot,
                WriteRequest::new(address, WriteMode::CreateNew, Publication::Staged)?,
            )
            .await
            .map(drop),
        Operation::CreateDirectory => lookup
            .execute_create_directory(slot, CreateDirectoryRequest::new(address)?)
            .await
            .map(drop),
        Operation::Remove => {
            lookup
                .execute_remove(slot, RemoveRequest::new(address, RemoveKind::NonDirectory)?)
                .await
        }
        Operation::Rename => {
            let target = slot.parent.entry().path().join(destination)?;
            let destination = EntrySlot {
                parent: slot.parent,
                name: destination,
                entry: None,
            };
            lookup
                .execute_rename(
                    slot,
                    destination,
                    RenameRequest::new(address, target, RenameMode::NoReplace)?,
                )
                .await
                .map(drop)
        }
        op => {
            let key = XattrName::parse("user.binding-contract")?;
            let action = match op {
                Operation::GetPermissions => A::GetPermissions,
                Operation::SetPermissions => A::SetPermissions(Permissions::new(0o600)?),
                Operation::ListXattrs => A::list_xattrs(NonZeroUsize::MIN),
                Operation::GetXattr => A::get_xattr(key, NonZeroUsize::MIN),
                Operation::SetXattr => A::set_xattr(key, vec![1], XattrWriteMode::Upsert),
                Operation::RemoveXattr => A::RemoveXattr(key),
                _ => return Err("operation missing from binding suite".into()),
            };
            lookup
                .execute_attribute(
                    Some((slot.parent, slot.name)),
                    entry()?,
                    AttributeRequest::new(address, action),
                )
                .await
                .map(drop)
        }
    };
    Ok(result)
}

fn assert_execution_failure(
    op: Operation,
    case: &str,
    result: Result<(), StorageError>,
    expected: Failure,
) -> TestResult {
    let error = result
        .err()
        .ok_or_else(|| format!("{op} accepted {case}"))?;
    assert_eq!(error.operation(), op, "{case}");
    assert_eq!(error.failure(), expected, "{op}: {case}");
    assert_eq!(error.outcome(), MutationOutcome::Unchanged, "{op}: {case}");
    Ok(())
}

fn path(value: &str) -> TestResult<StoragePath> {
    Ok(StoragePath::parse(value)?)
}

async fn put(storage: &dyn Storage, name: &str, bytes: &[u8]) -> TestResult {
    let mut writer = storage
        .write_file(
            crate::resolve::operand::WriteOperand::new(
                path(name)?.to_rooted_expr(),
                ah_vfs_storage::WriteMode::CreateNew,
                ah_vfs_storage::Publication::Staged,
            ),
            Default::default(),
        )
        .await?;
    writer.write_chunk(bytes).await?;
    writer.commit().await?;
    Ok(())
}

async fn collect(reader: &mut dyn StorageFileReader) -> TestResult<Vec<u8>> {
    let mut bytes = Vec::new();
    while let Some(chunk) = reader.next_chunk().await? {
        assert!(!chunk.is_empty());
        bytes.extend(chunk);
    }
    assert!(reader.next_chunk().await?.is_none());
    Ok(bytes)
}

async fn get(storage: &dyn Storage, name: &str) -> TestResult<Vec<u8>> {
    collect(
        storage
            .read_file(
                crate::resolve::operand::ReadOperand::new(
                    path(name)?.to_rooted_expr(),
                    ah_vfs_storage::ReadRange::FULL,
                ),
                Default::default(),
            )
            .await?
            .as_mut(),
    )
    .await
}

async fn mkdir(storage: &dyn Storage, name: &str) -> TestResult {
    storage
        .create_dir(
            crate::resolve::operand::CreateDirectoryOperand::new(
                path(name)?.to_rooted_expr(),
                crate::resolve::operand::CreateDirectoryMode::Recursive,
            ),
            Default::default(),
        )
        .await?;
    Ok(())
}

async fn exists(storage: &dyn Storage, name: &str) -> TestResult<bool> {
    match storage
        .stat_entry(
            crate::resolve::operand::StatOperand::new(
                path(name)?.to_rooted_expr(),
                crate::resolve::policy::SymlinkAction::Preserve,
            ),
            Default::default(),
        )
        .await
    {
        Ok(_) => Ok(true),
        Err(error) if error.failure() == Failure::Rejected(Rejection::NotFound) => Ok(false),
        Err(error) => Err(error.into()),
    }
}

/// Assert range reads, EOF and invalid-target semantics on fresh range-readable storage.
/// The fixture must be empty and isolated; the assertion writes files and dirs.
pub async fn assert_range_read_contract(storage: &dyn Storage) -> TestResult {
    let bytes: Vec<u8> = (0..150_000).map(|index| (index % 251) as u8).collect();
    put(storage, "file", &bytes).await?;
    put(storage, "empty", b"").await?;
    mkdir(storage, "dir").await?;

    for name in ["file", "empty"] {
        let content = if name == "file" {
            bytes.as_slice()
        } else {
            &[]
        };
        let eof = content.len() as u64;
        for (offset, length, expected) in [
            (u64::MAX, None, &[][..]),
            (u64::MAX, Some(0), &[][..]),
            (i64::MAX as u64 + 1, None, &[][..]),
            (eof, None, &[][..]),
            (eof + 1, None, &[][..]),
            (0, Some(0), &[][..]),
            (0, None, content),
            (7, Some(11), content.get(7..18).unwrap_or(&[])),
            (7, Some(u64::MAX - 7), content.get(7..).unwrap_or(&[])),
        ] {
            let mut reader = storage
                .read_file(
                    crate::resolve::operand::ReadOperand::new(
                        path(name)?.to_rooted_expr(),
                        ah_vfs_storage::ReadRange::new(offset, length)?,
                    ),
                    Default::default(),
                )
                .await?;
            let mut actual = Vec::new();
            while let Some(chunk) = reader.next_chunk().await? {
                assert!(!chunk.is_empty());
                assert!(chunk.len() <= storage.chunk_size().get());
                actual.extend(chunk);
            }
            assert!(reader.next_chunk().await?.is_none());
            assert!(reader.next_chunk().await?.is_none());
            assert_eq!(
                actual, expected,
                "{name}: offset={offset}, length={length:?}"
            );
        }
    }

    // Empty ranges still validate the target instead of hiding missing files or dirs.
    for (name, expected) in [
        ("missing", Failure::Rejected(Rejection::NotFound)),
        ("dir", Failure::Rejected(Rejection::IsDirectory)),
    ] {
        for length in [None, Some(0)] {
            let error = storage
                .read_file(
                    crate::resolve::operand::ReadOperand::new(
                        path(name)?.to_rooted_expr(),
                        ah_vfs_storage::ReadRange::new(u64::MAX, length)?,
                    ),
                    Default::default(),
                )
                .await
                .err()
                .ok_or("expected invalid read target")?;
            assert_eq!(error.failure(), expected);
        }
    }
    Ok(())
}

/// Assert recursive and exclusive directory creation on fresh writable storage.
/// The fixture must be empty and isolated; the assertion writes files and dirs.
pub async fn assert_create_dir_contract(storage: &dyn Storage) -> TestResult {
    use crate::resolve::operand::CreateDirectoryMode::{Recursive, Single};
    use ah_vfs_storage::MutationOutcome;

    let create = |operand: &str, mode| -> TestResult<_> {
        Ok(storage.create_dir(
            CreateDirectoryOperand::new(PathExpr::parse(operand)?, mode),
            Default::default(),
        ))
    };
    put(storage, "file", b"retained file").await?;
    mkdir(storage, "dir").await?;
    mkdir(storage, "dir/child").await?;

    for (name, mode, expected) in [
        (
            "file",
            Recursive,
            Failure::Rejected(Rejection::NotDirectory),
        ),
        (
            "file/child",
            Recursive,
            Failure::Rejected(Rejection::NotDirectory),
        ),
        (
            "file/child",
            Single,
            Failure::Rejected(Rejection::NotDirectory),
        ),
        ("file", Single, Failure::Rejected(Rejection::AlreadyExists)),
        ("file/", Single, Failure::Rejected(Rejection::NotDirectory)),
        (
            "file/",
            Recursive,
            Failure::Rejected(Rejection::NotDirectory),
        ),
        ("dir", Single, Failure::Rejected(Rejection::AlreadyExists)),
        (
            "missing/child",
            Single,
            Failure::Rejected(Rejection::NotFound),
        ),
    ] {
        let error = create(name, mode)?
            .await
            .err()
            .ok_or("expected create_dir failure")?;
        assert_eq!(error.failure(), expected, "{name}: {mode:?}");
        assert_eq!(
            error.outcome(),
            MutationOutcome::Unchanged,
            "{name}: {mode:?}"
        );
    }
    assert!(!exists(storage, "missing").await?);

    for name in ["/", "dir/"] {
        let before = storage
            .stat_entry(
                crate::resolve::operand::StatOperand::new(
                    path(name.trim_matches('/'))?.to_rooted_expr(),
                    crate::resolve::policy::SymlinkAction::Preserve,
                ),
                Default::default(),
            )
            .await?;
        let entry = create(name, Recursive)?.await?;
        assert_eq!(entry.path(), before.path());
        assert_eq!(entry.entry_type(), EntryType::Directory);
        assert_eq!(entry.modified(), before.modified());
        let after = storage
            .stat_entry(
                crate::resolve::operand::StatOperand::new(
                    path(name.trim_matches('/'))?.to_rooted_expr(),
                    crate::resolve::policy::SymlinkAction::Preserve,
                ),
                Default::default(),
            )
            .await?;
        assert_eq!(after.entry_type(), before.entry_type());
        assert_eq!(after.modified(), before.modified());

        let error = create(name, Single)?
            .await
            .err()
            .ok_or("expected existing directory")?;
        assert_eq!(error.failure(), Failure::Rejected(Rejection::AlreadyExists));
    }
    assert_eq!(get(storage, "file").await?, b"retained file");
    assert!(exists(storage, "dir/child").await?);

    let entry = create("new/parents/leaf", Recursive)?.await?;
    assert_eq!(entry.path(), &path("new/parents/leaf")?);
    assert_eq!(entry.entry_type(), EntryType::Directory);
    for name in ["new", "new/parents", "new/parents/leaf"] {
        assert_eq!(
            storage
                .stat_entry(
                    crate::resolve::operand::StatOperand::new(
                        path(name)?.to_rooted_expr(),
                        crate::resolve::policy::SymlinkAction::Preserve
                    ),
                    Default::default()
                )
                .await?
                .entry_type(),
            EntryType::Directory
        );
    }
    let entry = create("new/sibling", Single)?.await?;
    assert_eq!(entry.entry_type(), EntryType::Directory);
    let entry = create("before/../target/", Recursive)?.await?;
    assert_eq!(entry.path(), &path("target")?);
    assert!(exists(storage, "before").await?);
    assert!(!exists(storage, "before/target").await?);

    let error = create("visible/../file/child", Recursive)?
        .await
        .err()
        .ok_or("expected a non-directory parent")?;
    assert_eq!(error.failure(), Failure::Rejected(Rejection::NotDirectory));
    assert_eq!(error.outcome(), MutationOutcome::Unknown);
    assert!(exists(storage, "visible").await?);
    Ok(())
}

/// Assert directory cursor validation and ordered pagination on fresh writable storage.
/// The fixture must be empty and isolated; the assertion writes files and dirs.
pub async fn assert_pagination_contract(storage: &dyn Storage) -> TestResult {
    use ah_vfs_storage::PageSize;
    assert!(PageSize::new(0).is_err());
    assert!(PageSize::new(PageSize::MAX + 1).is_err());
    for after in ["", ".", "..", "a/b", "a\\b", "a:b", "a\0b"] {
        assert!(ah_vfs_storage::EntryName::parse(after).is_err());
    }
    for name in ["a", "b", "文件"] {
        put(storage, name, b"").await?;
    }
    let first = ListOperand::new(
        ListOperandTarget::Path(StoragePath::root().to_rooted_expr()),
        PageSize::new(1)?,
    );
    let page = storage.list_entries(&first, Default::default()).await?;
    assert_eq!(page.entries().len(), 1);
    assert_eq!(page.entries()[0].path(), &path("a")?);
    let cursor = page.continuation().ok_or("expected continuation")?;
    assert_eq!(cursor.after().as_str(), "a");
    let next = ListOperand::new(
        ListOperandTarget::Continue(cursor),
        PageSize::new(PageSize::MAX)?,
    );
    let page = storage.list_entries(&next, Default::default()).await?;
    assert_eq!(
        page.entries()
            .iter()
            .map(|entry| entry.path().as_str())
            .collect::<Vec<_>>(),
        ["b", "文件"]
    );
    assert!(page.continuation().is_none());
    Ok(())
}

/// Assert same-path rename semantics for the modes advertised by fresh writable storage.
/// The fixture must be empty and isolated; the assertion writes files and dirs.
pub async fn assert_same_path_rename_contract(
    storage: &dyn Storage,
    support: RenameSupport,
) -> TestResult {
    put(storage, "file", b"file content").await?;
    mkdir(storage, "dir").await?;
    mkdir(storage, "dir/child").await?;

    for mode in [RenameMode::NoReplace, RenameMode::Replace] {
        if !support.supports(mode) {
            continue;
        }
        for name in ["file", "dir"] {
            let before = storage
                .stat_entry(
                    crate::resolve::operand::StatOperand::new(
                        path(name)?.to_rooted_expr(),
                        crate::resolve::policy::SymlinkAction::Preserve,
                    ),
                    Default::default(),
                )
                .await?;
            let result = storage
                .move_entry(
                    RenameOperand::new(
                        (path(name)?).to_rooted_expr(),
                        (path(name)?).to_rooted_expr(),
                        mode,
                    )?,
                    Default::default(),
                )
                .await;
            if mode == RenameMode::Replace {
                let entry = result?;
                assert_eq!(entry.path(), before.path());
                assert_eq!(entry.entry_type(), before.entry_type());
                assert_eq!(entry.modified(), before.modified());
            } else {
                assert_eq!(
                    result
                        .err()
                        .ok_or("expected existing destination")?
                        .failure(),
                    Failure::Rejected(Rejection::AlreadyExists)
                );
            }
            let after = storage
                .stat_entry(
                    crate::resolve::operand::StatOperand::new(
                        path(name)?.to_rooted_expr(),
                        crate::resolve::policy::SymlinkAction::Preserve,
                    ),
                    Default::default(),
                )
                .await?;
            assert_eq!(after.path(), before.path());
            assert_eq!(after.entry_type(), before.entry_type());
            assert_eq!(after.modified(), before.modified());
            assert_eq!(get(storage, "file").await?, b"file content");
            assert!(exists(storage, "dir/child").await?);
        }

        let error = storage
            .move_entry(
                RenameOperand::new(
                    (path("missing")?).to_rooted_expr(),
                    (path("missing")?).to_rooted_expr(),
                    mode,
                )?,
                Default::default(),
            )
            .await
            .err()
            .ok_or("expected missing source")?;
        assert_eq!(error.failure(), Failure::Rejected(Rejection::NotFound));
        assert!(!exists(storage, "missing").await?);

        assert!(
            RenameOperand::new(
                StoragePath::root().to_rooted_expr(),
                path("target")?.to_rooted_expr(),
                mode
            )
            .is_err()
        );
        assert!(
            RenameOperand::new(
                path("source")?.to_rooted_expr(),
                StoragePath::root().to_rooted_expr(),
                mode
            )
            .is_err()
        );
    }
    Ok(())
}
